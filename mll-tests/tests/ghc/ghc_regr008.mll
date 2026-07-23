-- ghc_regr008: Where clause bindings used in guards

-- Where bindings referenced in guards
classify :: Int -> String
classify n
    | n < low   = "tiny"
    | n < mid   = "small"
    | n < high  = "medium"
    | n < huge  = "large"
    | otherwise = "huge"
  where
    low  = 10
    mid  = 100
    high = 1000
    huge = 10000

-- Where binding that is a function
categorize :: [Int] -> String
categorize xs
    | total == 0     = "empty"
    | avg < 0        = "negative"
    | avg < thresh   = "low"
    | otherwise      = "high"
  where
    total = length xs
    sumXs = foldl (+) 0 xs
    avg   = if total == 0 then 0 else sumXs `div` total
    thresh = 50

-- Multiple guards referencing same where binding
bmiCategory :: Number -> Number -> String
bmiCategory weight height
    | bmi < 18.5 = "underweight"
    | bmi < 25.0 = "normal"
    | bmi < 30.0 = "overweight"
    | otherwise  = "obese"
  where
    bmi = weight / (height * height)

-- Where binding used in guard and in body
describeList :: [a] -> String
describeList xs
    | n == 0    = "empty list"
    | n == 1    = "singleton list"
    | n <= 5    = "short list of " <> show n
    | otherwise = "long list of " <> show n
  where
    n = length xs

main :: IO ()
main = do
    assert (classify 5 == "tiny") "classify tiny"
    assert (classify 50 == "small") "classify small"
    assert (classify 500 == "medium") "classify medium"
    assert (classify 5000 == "large") "classify large"
    assert (classify 50000 == "huge") "classify huge"

    assert (categorize [] == "empty") "categorize empty"
    assert (categorize [10, 20, 30] == "low") "categorize low"
    assert (categorize [100, 200, 300] == "high") "categorize high"

    assert (bmiCategory 70.0 1.75 == "normal") "bmi normal"
    assert (bmiCategory 50.0 1.75 == "underweight") "bmi underweight"
    assert (bmiCategory 100.0 1.75 == "obese") "bmi obese"

    assert (describeList ([] :: [Int]) == "empty list") "empty"
    assert (describeList [1] == "singleton list") "singleton"
    assert (describeList [1, 2, 3] == "short list of 3") "short"
    assert (describeList [1..10] == "long list of 10") "long"

    putStrLn "ok"
