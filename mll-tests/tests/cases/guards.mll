classify :: Int -> String
classify n
    | n < 0     = "negative"
    | n == 0    = "zero"
    | otherwise = "positive"

safeHead :: [Int] -> String
safeHead [] = "empty"
safeHead (x:_) = show x

main :: IO ()
main = do
    assert (classify (-1) == "negative") "classify -1"
    assert (classify 0 == "zero") "classify 0"
    assert (classify 1 == "positive") "classify 1"
    assert (safeHead [42, 1] == "42") "safeHead [42,1]"
    assert (safeHead [] == "empty") "safeHead []"
