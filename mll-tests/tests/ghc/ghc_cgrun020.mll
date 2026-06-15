-- GHC cgrun020: Mutual recursion
-- Tests mutually recursive functions

isEven :: Integer -> Bool
isEven 0 = True
isEven n = isOdd (n - 1)

isOdd :: Integer -> Bool
isOdd 0 = False
isOdd n = isEven (n - 1)

main :: IO ()
main = do
    assert (isEven 0 == True) "0 even"
    assert (isOdd 0 == False) "0 not odd"
    assert (isEven 10 == True) "10 even"
    assert (isOdd 11 == True) "11 odd"
    assert (isEven 7 == False) "7 not even"
    assert (isOdd 8 == False) "8 not odd"
    putStrLn "ok"
