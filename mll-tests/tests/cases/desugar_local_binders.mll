import Data.IORef

scaled :: (Int -> Int) -> [Int] -> [Int]
scaled concatMap xs = [concatMap x | x <- xs]

mixed :: [Int] -> [Int]
mixed xs = [x + y | concatMap <- xs, y <- [concatMap, 10], let x = 1]

unwrap :: (Int -> Int) -> Maybe Int -> Int
unwrap error m = (\(Just x) -> error x) m

countdown :: Int -> Int -> Int
countdown error n = go n
  where
    go i
        | i > 0 = i + error

guarded :: Int -> Maybe Int -> Int
guarded error m
    | Just x <- m, x > error = x
    | otherwise = error

ranges :: Int -> Int -> Int -> Int -> [[Int]]
ranges enumFrom enumFromThen enumFromTo enumFromThenTo =
    [ take 3 [enumFrom ..]
    , take 3 [enumFromThen, 5 ..]
    , [1 .. 3] ++ [enumFromTo]
    , [enumFromThenTo, 3 .. 9]
    ]

bound :: Int -> IO Int
bound error = do
    Just x <- pure (Just 1)
    [y] <- pure [error]
    pure (x + y + error)

local :: Int -> Int
local n = error + concatMap 3
  where
    error = n * 2
    concatMap enumFrom = enumFrom + length [enumFrom .. enumFrom + 2]

branches :: Maybe Int -> Int
branches m = case m of
    Just error -> (\(concatMap : _) -> concatMap + error) [10, 20]
    Nothing -> let error = 5 in error

whole :: [Int] -> [Int]
whole error@(enumFromThen : _) = take 2 [enumFromThen, 3 ..] ++ error
whole [] = []

nested :: Int -> Int
nested error = (\error -> error * 2) (error + 1)

main :: IO ()
main = do
    assert (scaled (+ 1) [1, 2, 3] == [2, 3, 4]) "a parameter named concatMap"
    assert (mixed [1, 2] == [2, 11, 3, 11]) "a generator variable named concatMap"
    assert (unwrap (+ 1) (Just 1) == 2) "a parameter named error around a pattern lambda"
    assert (countdown 5 2 == 7) "a parameter named error around a where guard chain"
    assert (map (guarded 2) [Just 3, Just 1, Nothing] == [3, 2, 2]) "a parameter named error around a pattern guard"
    assert (ranges 7 1 7 1 == [[7, 8, 9], [1, 5, 9], [1, 2, 3, 7], [1, 3, 5, 7, 9]]) "parameters named like the range methods"
    r <- bound 10
    assert (r == 21) "a parameter named error around refutable binds"
    assert (local 4 == 14) "where bindings named error and concatMap"
    assert (branches (Just 1) == 11) "case and lambda binders"
    assert (branches Nothing == 5) "a let binder"
    assert (whole [1, 2] == [1, 3, 1, 2]) "an as-pattern binder"
    assert (nested 1 == 4) "a binder shadowing a binder"
    error <- newIORef (0 :: Int)
    modifyIORef error (+ 5)
    concatMap <- readIORef error
    assert ([concatMap + x | x <- [1 .. 3]] == [6, 7, 8]) "do-bound names"
    print (scaled (+ 1) [1, 2, 3], mixed [1, 2], unwrap (+ 1) (Just 1), countdown 5 2)
    print (map (guarded 2) [Just 3, Just 1, Nothing], ranges 7 1 7 1, r)
    print (local 4, branches (Just 1), branches Nothing, whole [1, 2], nested 1)
    print [concatMap + x | x <- [1 .. 3]]
