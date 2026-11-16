-- Pattern bindings `pat = e` (Haskell 2010 section 4.4.3.2) at the top
-- level and in let, do-let and where groups: any pattern, with guards and
-- a where block, the variables in scope for the right-hand side and the
-- sibling bindings, and LAZY — the match happens when a variable of the
-- pattern is first demanded, and never if none is. A top-level pattern
-- binding's variables carry type signatures, like every top-level
-- binding of mata-ll. (F12h)
data P = P Int String deriving Show

lo, hi :: Int
(lo, hi) = (1, 9)

k :: Int
Just k = Just 5

p, q :: Int
[p, q] = [2, 3]

hd :: Int
tl :: [Int]
hd : tl = [10, 20, 30]

whole :: (Int, [Int])
inner :: Int
whole@(inner, _ : _) = (7, [8])

num :: Int
name :: String
P num name = P 4 "four"

never :: Int
Just never = Nothing

unused, used :: Int
(unused, used) = (undefined, 2)

evens, odds :: [Int]
(evens, odds) = (0 : map (+ 1) odds, map (+ 1) evens)

idA :: a -> a
wrap :: a -> [a]
(idA, wrap) = (id, \x -> [x])

chosen :: Int
Just chosen
  | lo > hi = Just lo
  | otherwise = Just bigger
  where bigger = hi * 100

third :: Int
(_, _, third) = (never, unused, 3)

total :: Int
total = lo + hi + k + p + q + hd + inner + num + used + chosen + third

inLet :: Int -> (Int, Int, Int, [Int], Int)
inLet n =
  let Just a = Just (n + 1)
      [b, c] = [a * 2, a * 3]
      x : xs = [c, b, a]
      w@(_, e) = (xs, x + 1)
  in (a, b, c, fst w, e)

inWhere :: Int -> (Int, Int, String, Int)
inWhere n = (r, s, t, u)
  where
    Just r = Just (n * 2)
    (s, _) : _ = [(r + 1, r), (0, 0)]
    P u t
      | n > 100 = P 0 "big"
      | otherwise = P small "small"
      where small = n - 1

lazyLet :: Int
lazyLet = let Just j = Nothing :: Maybe Int in 5

lazyWhere :: Int -> Int
lazyWhere n = if n > 0 then n else boom
  where
    Just boom = Nothing
    [] = [n]

shared :: Int -> Int
shared n = a + b
  where
    (a, b) = (n, a * 2); Just _ = Just a

main :: IO ()
main = do
  print (lo, hi, k, p, q)
  print (hd, tl, whole, inner)
  print (num, name, used, third)
  print (take 4 evens, take 4 odds)
  print (idA 1, idA "s", wrap True)
  print (chosen, total)
  print (inLet 1)
  print (inWhere 3)
  print (lazyLet, lazyWhere 4, shared 5)
  let Just m = Just 11
      (n1, n2) = (m + 1, m + 2)
      [] = [m]
  print (m, n1, n2)
  let Just missing = Nothing :: Maybe Int
  print 12
