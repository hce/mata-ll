-- A declaration is `(funlhs | pat) rhs` (Haskell 2010 section 4.4.3), the
-- same in every declaration group: the module's top level, where and let
-- groups, class bodies and instance bodies. Pins the three function heads
-- (`var apat…`, `pat varop pat` with full patterns as operands,
-- `( funlhs ) apat…`) with guards and several equations, and operands
-- that are cons patterns where the operator's fixity lets them be. The
-- pattern bindings are in pattern_bindings.mll, the group-scoped
-- signatures and fixities in local_operator_scope.mll. (F12h, Q31, Q61,
-- U22, X3)
data V = V Int Int deriving Show

data T = L | N T Int T deriving Show

data A = A deriving Show

infixl 6 |+|
(|+|) :: V -> V -> V
V a b |+| V c d = V (a + c) (b + d)

infixl 7 |*|
(|*|) :: Int -> V -> V
k |*| V a b = V (k * a) (k * b)

(|-|) :: V -> V -> V
V a b |-| V c d = V (a - c) (b - d)

pairPlus :: (Int, Int) -> (Int, Int) -> (Int, Int)
(x, y) `pairPlus` (u, v) = (x + u, y + v)

firstOr :: [Int] -> Int -> Int
(a : _) `firstOr` _ = a
[] `firstOr` d = d

orElse :: Maybe Int -> Int -> Int
Just x `orElse` _ = x
Nothing `orElse` d = d

(<?>) :: Maybe Int -> Int -> Int
Just x <?> _ = x
Nothing <?> d = d

classify :: Int -> String -> String
0 `classify` _ = "zero"
n `classify` "sign"
  | n < 0 = "negative"
  | otherwise = "positive"
_ `classify` other = other

whole :: [Int] -> Int -> (Int, [Int])
xs@(x : _) `whole` n = (x + n, xs)
[] `whole` n = (n, [])

insert :: Int -> T -> T
insert x L = N L x L
insert x t@(N l v r)
  | x < v = N (insert x l) v r
  | x > v = N l v (insert x r)
  | otherwise = t

has :: T -> Int -> Bool
N l v r `has` x
  | x == v = True
  | x < v = l `has` x
  | otherwise = r `has` x
L `has` _ = False

infixl 4 +++
(+++) :: [Int] -> [Int] -> [Int]
x : xs +++ ys = x : (xs +++ ys)
[] +++ ys = ys

infixr 5 ***
(***) :: Int -> [Int] -> [Int]
k *** x : xs = k * x : k *** xs
_ *** [] = []

(|^|) :: Int -> Int -> Int
-1 |^| k = k
k |^| -1 = k + 100
_ |^| _ = 0

op :: Int -> Int -> Int -> Int
(x `op` y) z = x + y * z

(.+.) :: (Int -> Int) -> (Int -> Int) -> Int -> Int -> Int
(f .+. g) a b = f a + g b

op2 :: (Int -> Int) -> (Int -> Int) -> Int -> Int -> Int
((h `op2` k) a) b = h a * k b

twice :: Int -> Int
(twice) x = x * 2

pick :: Bool -> Int -> Int -> Int
(pick True) x _ = x
(pick False) _ y = y

class Combine a where
  (<#>), (<~>) :: a -> a -> a
  infixr 5 <#>
  x <~> _ = x
  scale :: Int -> a -> a

instance Combine V where
  V a b <#> V c d = V (a * c) (b * d)
  k `scale` V a b = V (k * a) (k * b)

instance Combine T where
  L <#> t = t
  N l v r <#> t = N l v (r <#> t)
  N l v _ <~> N _ w r = N l (v + w) r
  t <~> _ = t
  (scale 0) _ = L
  k `scale` N l v r = N (k `scale` l) (k * v) (k `scale` r)
  _ `scale` L = L

class Sized a where
  (+>) :: [Int] -> a -> Int
  infixl 4 +>

instance Sized A where
  x : _ +> A = x
  [] +> A = 0

instance Eq V where
  V a b == V c d = a == c && b == d

instance Eq A where
  A == A = True

main :: IO ()
main = do
  print (V 1 2 |+| V 3 4 |+| V 5 6)
  print (2 |*| V 1 2 |+| V 10 10)
  print (V 9 9 |-| V 1 2)
  print ((1, 2) `pairPlus` (3, 4))
  print ([7, 8] `firstOr` 0, [] `firstOr` 0)
  print (Just 3 `orElse` 0, Nothing `orElse` 9)
  print (Just 3 <?> 0, Nothing <?> 9)
  print (0 `classify` "sign", 5 `classify` "sign", (-5) `classify` "sign", 5 `classify` "x")
  print ([4, 5] `whole` 1, [] `whole` 1)
  let t = foldr insert L [5, 3, 8, 1]
  print t
  print (t `has` 3, t `has` 4)
  print ([1, 2] +++ [3], [] +++ [4], 2 *** [1, 2, 3])
  print ((-1) |^| 3, 5 |^| (-1), 2 |^| 2)
  print ((1 `op` 2) 3, (succ .+. pred) 10 20, (succ `op2` pred) 1 5)
  print (twice 4, pick True 1 2, pick False 1 2)
  print (V 2 3 <#> V 4 5, V 2 3 <~> V 4 5, 3 `scale` V 1 2)
  print (N L 1 L <#> N L 2 L <#> L, N L 1 L <~> N L 2 (N L 3 L), L <~> N L 1 L)
  print (scale 0 t, 2 `scale` N L 1 (N L 2 L))
  print (V 1 2 == V 1 2, V 1 2 == V 2 1, V 1 2 /= V 2 1, A == A)
  print ([7, 8] +> A, [] +> A)
