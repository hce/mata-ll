-- GHC ds010: Do-notation desugaring with let, bind, sequence

safeDiv :: Int -> Int -> Maybe Int
safeDiv _ 0 = Nothing
safeDiv x y = Just (x `div` y)

safeHead :: [a] -> Maybe a
safeHead []    = Nothing
safeHead (x:_) = Just x

-- do-notation with multiple binds and let
compute :: Int -> Int -> Int -> Maybe Int
compute a b c = do
    x <- safeDiv a b
    let y = x + 1
    z <- safeDiv y c
    return (z * 2)

-- do-notation with sequence: use >>= ignoring the bound value
sequenced :: Maybe Int
sequenced = Just 1 >>= (\a -> Just 2 >>= (\b -> Just (a + b - a - b + 3)))

-- do-notation that short-circuits: use case to simulate
shortCircuit :: Maybe Int
shortCircuit = case Just 10 of
    Nothing -> Nothing
    Just x  -> case (Nothing :: Maybe Int) of
        Nothing -> Nothing
        Just y  -> Just (x + y + 1)

-- cross product via list comprehension
pairs :: [(Int, Int)]
pairs = [(x, y) | x <- [1, 2, 3], y <- [10, 20]]

-- filter evens using filter function
evens :: [Int]
evens = filter (\x -> x `mod` 2 == 0) [1..10]

main :: IO ()
main = do
    assert (compute 10 2 3 == Just 4) "compute ok"
    assert (compute 10 0 3 == Nothing) "compute div0 b"
    assert (compute 10 2 0 == Nothing) "compute div0 c"

    assert (sequenced == Just 3) "sequenced"
    assert (shortCircuit == Nothing) "short circuit"

    assert (pairs == [(1,10),(1,20),(2,10),(2,20),(3,10),(3,20)]) "pairs"
    assert (evens == [2,4,6,8,10]) "evens"

    -- let in do: use >>= instead of nested do
    let result = safeHead [5, 6, 7] >>= (\a -> let b = a * 3 in safeDiv b 5)
    assert (result == Just 3) "let in do"

    putStrLn "ok"
