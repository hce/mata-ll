-- Bang patterns `!p` (on in GHC's default language): parameters of
-- functions, methods and local functions, inside tuple, constructor, cons
-- and as-patterns, on constructor and newtype patterns, with guards,
-- lambdas, case alternatives, do binds, generators, and strict let and
-- where bindings; a banged `False`, `Nothing` and `[]` match like any
-- value. `!` stays an operator wherever it is not a prefix: `xs ! 0`,
-- `xs!1`, a section, `(!)`. The evaluation itself is pinned by the runtime
-- tests bang_pattern_evaluates_what_it_matches. (F12a)
data T = T Int Int deriving Show

newtype N = N Int

infixl 9 !

(!) :: [a] -> Int -> a
xs ! n = xs !! n

sumTo :: Int -> Int -> Int
sumTo !acc 0 = acc
sumTo !acc n = sumTo (acc + n) (n - 1)

k :: Int -> Int -> Int
k !_ y = y

two :: Int -> Int -> Int
two !x 0 = 1
two x n = x + n

inTuple :: (Int, Int) -> Int
inTuple (!a, _) = a

inCon :: T -> Int
inCon (T !a !_) = a

onCon :: Maybe Int -> Int
onCon !(Just x) = x
onCon !Nothing = 0

asBang :: [Int] -> Int
asBang !xs@(x : _) = x + length xs
asBang !_ = 0

inner :: [Int] -> Int
inner (_ : !rest) = length rest
inner [] = 0

ntBang :: N -> Int
ntBang !(N n) = n + 1

guarded :: Int -> Int -> Int
guarded !x y
  | y > 0 = x + y
  | otherwise = y

boolBang :: Bool -> Maybe Int -> [Int] -> Int
boolBang !b !m !l = if b then 1 else 2

whereBang :: Int -> Int -> Int
whereBang x y
  | y > 0 = z + 1
  | otherwise = z
  where !z = x * 2

mean :: [Int] -> Int
mean xs = go 0 0 xs
  where
    go !s !n [] = if n == 0 then 0 else s `div` n
    go !s !n (y : ys) = go (s + y) (n + 1) ys

class Sz a where
  sz :: a -> Int -> Int

instance Sz T where
  sz !(T a _) !n = a + n

main :: IO ()
main = do
  print (sumTo 0 100000)
  print (k 3 4, two 1 0, two 1 2)
  print (inTuple (1, undefined), inCon (T 5 6))
  print (onCon (Just 3), onCon Nothing)
  print (asBang [1, 2, 3], asBang [])
  print (inner [1, 2, 3], ntBang (N 4))
  print (guarded 1 2, guarded 1 0)
  print (boolBang False Nothing [], boolBang True (Just 1) [1])
  print (whereBang 2 1, whereBang 2 0)
  print (mean [1 .. 100])
  print (sz (T 1 2) 3)
  let !x = 3 + 4 :: Int
  print x
  let !(a, b) = (1 :: Int, undefined :: Int)
      !_ = 5 :: Int
      (!c, d) = (undefined :: Int, 6 :: Int)
      !f = \v -> v + 1 :: Int
  print (a, f 1)
  print ((\ !p -> p + 1) (1 :: Int))
  print (case (1 :: Int, 2 :: Int) of (!p, !q) -> p + q)
  print (case False of
           !t | t -> 1 :: Int
           _ -> 2)
  !m <- return (9 :: Int)
  (!n, _) <- return (10 :: Int, undefined :: Int)
  print (m + n)
  print [y | !y <- [1, 2, 3 :: Int], y > 1]
  print ([4, 5, 6 :: Int] ! 0, [4, 5, 6 :: Int]!1, map ([7, 8 :: Int] !) [0, 1], (!) [9 :: Int] 0)
  print (let !u = 1 :: Int
             !w = u + 1
         in u + w)
  print (rec 5)
  where
    rec :: Int -> Int
    rec n = let !r = if n == 0 then 0 else n + rec (n - 1) in r
