countdown 0 = 0
countdown n = n + countdown (n - 1)

unrelated :: Int
unrelated = 7

countdown :: Int -> Int

infixl 6 +++, ***

(+++) :: Int -> Int -> Int
(+++) 0 b = b
a +++ b = a + b

(***) :: Int -> Int -> Int
a *** b = a * b

data Shape = Circle { size :: Int } | Square { size :: Int, corner :: Int }
    deriving (Show)

data Token = Token Int
    deriving (Show)

class Describe a where
    describe :: a -> Int -> String
    label :: a -> String
    describe _ 0 = "zero"
    describe x _ = label x
    label _ = "thing"

instance Describe Shape where
    label (Circle _) = "circle"
    label (Square _ _) = "square"

instance Describe Int where
    describe 0 0 = "both zero"
    describe _ 1 = "one"
    describe n _ = label n
    label 0 = "nought"
    label _ = "int"

instance Describe Bool where

shadow :: Int -> Int
shadow x = (\x -> x + 1) x + (case x * 2 of
    x -> x) + (let g x = x * 3 in g x)

nested :: Int -> Int -> Int
nested x y = (\x -> \y -> x - y) y x

sameName :: Int -> Int
sameName sameName = sameName + 1

plus :: Int -> Int -> Int
plus `plus` other = plus + other

whole :: Maybe Int -> Int
whole m@(Just n) = n + count m
  where
    count (Just n) = n
    count Nothing = 0
whole Nothing = 0

pairs :: [(Int, Int)]
pairs = [(x, y) | x <- [1, 2], y <- [x, x * 10], x <- [y + 1]]

main :: IO ()
main = do
    print (countdown 4 + unrelated)
    print (0 +++ 5, 2 +++ 5, 2 +++ 3 *** 4)
    print (size (Circle 3), size (Square 4 1), corner (Square 4 1))
    print (Token 1)
    putStrLn (describe (Circle 1) 0)
    putStrLn (describe (Square 1 2) 5)
    putStrLn (describe (0 :: Int) 0)
    putStrLn (describe (7 :: Int) 1)
    putStrLn (describe (0 :: Int) 2)
    putStrLn (describe True 3)
    print (shadow 5)
    print (nested 10 3)
    print (sameName 1)
    print (plus 1 2)
    print (whole (Just 4), whole Nothing)
    print pairs
    x <- pure 1
    x <- pure (x + 1)
    (x, y) <- pure (x, x * 2)
    print (x + y)
