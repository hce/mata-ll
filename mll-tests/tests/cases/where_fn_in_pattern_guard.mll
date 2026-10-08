f :: Maybe Int -> Int
f m
  | Just x <- m = go x
  | otherwise = 0
  where go y = y + 1

g :: [Int] -> Int
g xs
  | (y:_) <- xs, y > 0 = scale y
  | [] <- xs = base
  | otherwise = negate (scale (head xs))
  where
    scale z = z * factor
    factor = 10
    base = -1

h :: Int -> Maybe Int -> String
h d m
  | Just x <- m = show (combine x)
  | otherwise = fallback
  where
    combine 0 = d
    combine x = x + d
    fallback = "none"

main :: IO ()
main = do
    print (f (Just 4))
    print (f Nothing)
    print (g [3, 4])
    print (g [])
    print (g [-2])
    putStrLn (h 5 (Just 0))
    putStrLn (h 5 (Just 2))
    putStrLn (h 5 Nothing)
