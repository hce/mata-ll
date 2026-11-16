-- Lazy patterns `~p` (Haskell 2010 section 3.17): matching always succeeds
-- and the pattern is matched when one of its variables is first demanded.
-- Every position a pattern can stand in: function and method parameters
-- (single and multiple clauses, under a tail-recursive loop, with guards
-- and a where group, shadowed by a where binding), nested inside tuple,
-- constructor, cons and as-patterns, lambdas, case alternatives with
-- guards that fall through, pattern guards, do binds, comprehension
-- generators, and let, where and top-level pattern bindings. Each use
-- passes `undefined` where the match must not happen. (F12a)
data Box = Box Int (Int, Int)

class Describe a where
  describe :: a -> (Int, Int) -> Int

instance Describe Box where
  describe (Box n _) ~(a, _) = if n > 0 then a else n

h :: (Int, Int) -> Int -> Int
h ~(_, _) 0 = 0
h ~(a, _) n = a + n

loop :: (Int, Int) -> Int -> Int -> Int
loop ~(_, _) acc 0 = acc
loop p acc n = loop p (acc + 1) (n - 1)

count :: (Int, Int) -> Int -> Int -> Int
count ~(a, b) acc n = if n == 0 then acc + a else count (a, b) (acc + 1) (n - 1)

g2 :: (Int, Int) -> Int -> Int
g2 ~(a, b) n
  | n > 0 = a + w
  | otherwise = 0
  where w = b * 2

sh :: (Int, Int) -> Int
sh ~(a, b) = a + b
  where b = 10

nest :: (Int, (Int, Int)) -> Int -> Int
nest (x, ~(y, z)) n = if n > 0 then x + y + z else x

inJust :: Maybe (Int, Int) -> Int
inJust (Just ~(a, _)) = 1
inJust Nothing = 0

lcons :: [Int] -> Int -> Int
lcons ~(x : xs) n = if n > 0 then x + length xs else n

asLazy :: (Int, Int) -> Int -> Int
asLazy p@(~(a, _)) n = if n > 0 then a + fst p else n

viaGuard :: Int -> Int
viaGuard n
  | ~(a, b) <- pairOf n, n > 0 = a + b
  | otherwise = 0

pairOf :: Int -> (Int, Int)
pairOf n = if n > 0 then (n, n) else undefined

plain :: Int -> Int -> Int
plain ~x ~_ = x

mixed :: (Int, Int) -> Int -> Int
mixed ~(!a, b) n = if n > 0 then b else n

ta, tb :: Int
~(ta, tb) = (3, 4)

tc :: Int
td, te :: Int
(tc, ~(td, te)) = (5, undefined)

main :: IO ()
main = do
  print (describe (Box 0 (1, 2)) undefined, describe (Box 1 (1, 2)) (7, undefined))
  print (h undefined 0, h (5, undefined) 2)
  print (loop undefined 0 100000)
  print (count (2, undefined) 0 1000)
  print (g2 undefined 0, g2 (1, 2) 1)
  print (sh (1, undefined))
  print (nest (1, undefined) 0, nest (1, (2, 3)) 1)
  print (inJust (Just undefined), inJust Nothing)
  print (lcons [] 0, lcons [4, 5, 6] 1)
  print (asLazy undefined 0, asLazy (3, 4) 1)
  print (viaGuard 0, viaGuard 2)
  print (plain 1 undefined)
  print (mixed undefined 0, mixed (1, 2) 1)
  print (ta, tb, tc)
  print (case (undefined :: (Int, Int)) of
           ~(a, _) | ta > 5 -> a
                   | tb > 5 -> a + 1
           _ -> 2)
  print (case (4 :: Int, 5 :: Int) of
           ~(a, b) | a > 5 -> a
                   | b > 4 -> b
           _ -> 2)
  print (map (\ ~(a, _) -> 3 :: Int) [undefined, undefined])
  ~(u, v) <- return (undefined :: (Int, Int))
  (w, ~(_, _)) <- return (8 :: Int, undefined :: (Int, Int))
  print w
  print (length [1 :: Int | ~(_, _) <- [undefined :: (Int, Int), undefined]])
  print [a | ~(a, _) <- [(1 :: Int, undefined :: Int), (2, undefined)], a > 1]
  let ~(p, q) = undefined :: (Int, Int)
      (r, ~(s, t)) = (1 :: Int, undefined :: (Int, Int))
      go ~(a, b) k = if k == (0 :: Int) then 0 else a + b + go (a, b) (k - 1) :: Int
  print r
  print (go undefined 0, go (1, 2) 3)
  print (wh 1)
  where
    wh :: Int -> Int
    wh n = n + m
      where
        ~(m, _) = (n * 2, undefined :: Int)
