add :: Int -> Int -> Int
add a b = a + b

abs' :: Int -> Int
abs' n = if n < 0 then 0 - n else n

main :: IO ()
main = do
    assert (add 1 2 == 3) "add 1 2 should be 3"
    assert (add 0 0 == 0) "add 0 0 should be 0"
    assert (add (-1) 1 == 0) "add (-1) 1 should be 0"
    assert (abs' (-5) == 5) "abs' (-5) should be 5"
    assert (abs' 5 == 5) "abs' 5 should be 5"
    assert (abs' 0 == 0) "abs' 0 should be 0"
