-- Rank-2 polymorphism tests

-- A function that requires a polymorphic argument
applyBoth :: (forall a. a -> a) -> Integer -> String -> (Integer, String)
applyBoth f x y = (f x, f y)

-- Using it with id (which is polymorphic)
test1 :: (Integer, String)
test1 = applyBoth id 42 "hello"

-- A rank-2 function that applies a polymorphic function to a list and a maybe
applyToEach :: (forall a. a -> a) -> [Integer] -> [Integer]
applyToEach f xs = map f xs

test2 :: [Integer]
test2 = applyToEach id [1, 2, 3]

-- Rank-2 with multiple uses at different types in the body
polyPair :: (forall a. a -> a) -> (Integer, String)
polyPair f = (f 10, f "world")

test3 :: (Integer, String)
test3 = polyPair id

-- runST still works (regression check)
sumST :: [Integer] -> Integer
sumST xs = runST (do
    acc <- newSTArray 1 0
    sumGo acc xs
    readSTArray acc 0)
  where
    sumGo acc [] = pure ()
    sumGo acc (x:xs) = do
      old <- readSTArray acc 0
      writeSTArray acc 0 (old + x)
      sumGo acc xs

test4 :: Integer
test4 = sumST [1, 2, 3, 4, 5]

main :: IO ()
main = do
    putStrLn $ show test1
    putStrLn $ show test2
    putStrLn $ show test3
    putStrLn $ show test4
