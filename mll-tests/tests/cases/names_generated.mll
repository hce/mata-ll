data Color = Red | Green deriving (Show, Eq, Ord)

data Shape = Circle Int | Square Int deriving (Show, Eq, Ord)

data Tag = Tag Int

instance Show Tag where
    show (Tag n) = "Tag#" <> show n

class Describe a where
    describe :: a -> String
    describe _ = "thing"
    bsLength :: a -> Int

instance Describe Int where
    bsLength n = n

instance Describe a => Describe [a] where
    describe xs = "[" <> mconcat (map describe xs) <> "]"
    bsLength xs = sum (map bsLength xs)

show_Color :: Color -> String
show_Color _ = "user show_Color"

eq_Color :: Color -> Color -> Bool
eq_Color _ _ = True

ord_compare__Shape :: Shape -> Shape -> Int
ord_compare__Shape _ _ = 0

showsPrec_Shape :: Int -> Int
showsPrec_Shape n = n + 100

show_Tag :: Tag -> String
show_Tag _ = "user show_Tag"

show_Int :: Int -> String
show_Int _ = "user show_Int"

eq_Int :: Int -> Int -> Bool
eq_Int _ _ = False

ord_lt__Int :: Int -> Int -> Bool
ord_lt__Int a b = a > b

eq_LInt :: [Int] -> [Int] -> Bool
eq_LInt _ _ = True

show_LInt :: [Int] -> String
show_LInt _ = "user show_LInt"

describe_Int :: Int -> String
describe_Int _ = "user describe_Int"

count :: Eq a => a -> [a] -> Int
count _ [] = 0
count x (y : ys) = (if x == y then 1 else 0) + count x ys

count_IntTLIntTInt :: Int -> [Int] -> Int
count_IntTLIntTInt _ _ = 99

deep :: Describe a => Int -> a -> String
deep 0 x = describe x <> show (bsLength x)
deep n x = deep (n - 1) [x, x]

nest :: (Eq a, Show a) => Int -> a -> a -> String
nest 0 x y = show x <> (if x == y then "==" else "/=") <> show y
nest n x y = nest (n - 1) [x] [y]

eq_dictform_LUnit :: Int -> Int
eq_dictform_LUnit n = if n > 100 then n else eq_dictform_LUnit (n * 2)

show_dictform_LUnit :: Int -> Int
show_dictform_LUnit n = if n > 100 then n else show_dictform_LUnit (n * 3)

not_ :: Int -> Bool -> Bool
not_ n b = if n > 0 then not_ (n - 1) b else b

hashmap_size :: Int -> Int
hashmap_size n = if n > 100 then n else hashmap_size (n * 2)

foldl_prime :: Int -> Int
foldl_prime n = n * 2

main :: IO ()
main = do
    k <- pure (3 :: Int)
    print (Red, Red == Green, Red < Green, compare (Circle k) (Square 1))
    print (Just (Circle k), [Circle 1, Square 2], Tag k)
    putStrLn (show_Color Red <> " " <> show (eq_Color Red Green) <> " " <> show_Tag (Tag 1))
    print (ord_compare__Shape (Circle 1) (Circle 2), showsPrec_Shape k)
    print (k, [k, 4], k == 3, k < 4, [k] == [3])
    putStrLn (show_Int k <> " " <> show (eq_Int k k) <> " " <> show (ord_lt__Int k 4))
    putStrLn (show (eq_LInt [k] [4]) <> " " <> show_LInt [k])
    putStrLn (describe k <> " " <> describe [k, k] <> " " <> describe_Int k)
    print (count k [3, 2, 3], count_IntTLIntTInt k [3], count "a" ["a"])
    putStrLn (deep 2 k)
    putStrLn (nest 2 k 3 <> " " <> nest 2 k 4)
    print (eq_dictform_LUnit k, show_dictform_LUnit k)
    print (map not [True, not_ k False], hashmap_size k, foldl_prime k, foldl' (+) 0 [k, 1])
