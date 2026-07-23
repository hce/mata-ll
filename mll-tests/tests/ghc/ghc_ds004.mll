-- GHC ds004: Where clause scoping
-- Tests that where bindings have correct scoping

f :: Int -> Int
f x = a + b
  where
    a = x * 2
    b = 1

g :: Int -> Int -> Int
g x y = p + q
  where
    p = x * x
    q = y * y

-- Recursive list sum
h :: [Int] -> Int
h [] = 0
h (x:xs) = x + h xs

-- Where with helper function
nested :: Int -> Int
nested x = inner (x + offset)
  where
    offset = 10
    inner y = y * 3

main :: IO ()
main = do
    assert (f 5 == 11) "f 5"
    assert (f 0 == 1) "f 0"
    assert (g 3 4 == 25) "g 3 4"
    assert (h [1, 2, 3] == 6) "h sum"
    assert (h [] == 0) "h empty"
    assert (nested 5 == 45) "nested where"
    putStrLn "ok"
