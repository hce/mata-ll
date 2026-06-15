-- GHC ds007: Complex guard patterns
-- Guards in case expressions and in function definitions

data Sign = Neg | Zero | Pos
    deriving (Show, Eq)

signOf :: Integer -> Sign
signOf n
    | n < 0     = Neg
    | n == 0    = Zero
    | otherwise = Pos

-- Guards inside case
describeSign :: Sign -> String
describeSign s = case s of
    Neg  -> "negative"
    Zero -> "zero"
    Pos  -> "positive"

-- Guards in case on Integer
category :: Integer -> String
category n
    | n == 0    = "none"
    | n < 0     = "below"
    | n < 10    = "low"
    | n < 100   = "mid"
    | otherwise = "high"

-- Multiple guard clauses per equation
bmi :: Number -> String
bmi b
    | b < 18.5  = "underweight"
    | b < 25.0  = "normal"
    | b < 30.0  = "overweight"
    | otherwise = "obese"

-- Guards + where
quadrant :: Integer -> Integer -> String
quadrant x y
    | posX && posY  = "I"
    | negX && posY  = "II"
    | negX && negY  = "III"
    | posX && negY  = "IV"
    | otherwise     = "axis"
  where
    posX = x > 0
    negX = x < 0
    posY = y > 0
    negY = y < 0

main :: IO ()
main = do
    assert (signOf (0 - 3) == Neg)  "sign neg"
    assert (signOf 0  == Zero) "sign zero"
    assert (signOf 7  == Pos)  "sign pos"

    assert (describeSign Neg == "negative") "desc neg"
    assert (describeSign Zero == "zero")    "desc zero"

    assert (category 0   == "none")  "cat none"
    assert (category 5   == "low")   "cat low"
    assert (category 50  == "mid")   "cat mid"
    assert (category 200 == "high")  "cat high"

    assert (bmi 17.0 == "underweight") "bmi under"
    assert (bmi 22.0 == "normal")      "bmi normal"
    assert (bmi 35.0 == "obese")       "bmi obese"

    assert (quadrant 1 1 == "I")         "quad I"
    assert (quadrant (0-1) (0-1) == "III") "quad III"
    assert (quadrant 1 (0-1) == "IV")   "quad IV"

    putStrLn "ok"
