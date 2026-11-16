-- Strict constructor fields `!T` (Haskell 2010 section 4.2.1): positional,
-- record and GADT-syntax declarations, polymorphic, recursive and
-- existential types, next to lazy fields that still take `undefined`.
-- Every way a constructor is applied: saturated, partial, first-class
-- (map, zipWith, an argument), record construction, record update, a
-- derived Functor; a partial application and an unused value evaluate
-- nothing. The evaluation itself is pinned by the runtime tests
-- strict_field_is_evaluated_by_every_construction. (F12b)
data P = P !Int !Int deriving (Show, Eq, Ord)

data Q a = Q !a [a] | R { rx :: !Int, ry :: Int } deriving Show

data L = L Int !Int

data Mixed = MA { mv :: !Int, mw :: Int, pad :: Int } | MB { mv :: Int, mw :: Int, pad :: Int }

data Tree a = Leaf | Node !(Tree a) a !(Tree a) deriving Show

data Shape = forall a. Show a => Shape !a Int

data G where
  MkG :: !Int -> Int -> G
  MkH :: Int -> !Int -> G

data F a = F !a Int deriving (Show, Functor)

data Wide = Wide { w1 :: Int, w2 :: !Int, w3 :: Int, w4 :: !String } deriving (Show, Eq)

fstL :: L -> Int
fstL (L a _) = a

sndL :: L -> Int
sndL (L _ b) = b

sndP :: P -> Int
sndP (P _ b) = b

gsnd :: G -> Int
gsnd (MkG _ b) = b
gsnd (MkH a _) = a

insert :: Int -> Tree Int -> Tree Int
insert x Leaf = Node Leaf x Leaf
insert x t@(Node l v r)
  | x < v = Node (insert x l) v r
  | x > v = Node l v (insert x r)
  | otherwise = t

toList :: Tree a -> [a]
toList Leaf = []
toList (Node l v r) = toList l ++ [v] ++ toList r

lazyOk :: Int -> Int
lazyOk n = fstL (L n 2) + n

sumP :: Int -> P -> P
sumP 0 acc = acc
sumP n (P a b) = sumP (n - 1) (P (a + n) (b + 1))

shapeN :: Shape -> Int
shapeN (Shape _ n) = n

apply2 :: (Int -> Int -> P) -> P
apply2 f = f 7 8

main :: IO ()
main = do
  print (P 1 2, P 1 2 == P 1 2, compare (P 1 2) (P 1 3))
  print (Q (3 :: Int) [4, 5])
  print (R { rx = 1, ry = 2 } :: Q Int)
  print (case R { rx = 1, ry = undefined } of
           R a _ -> a
           Q a _ -> a)
  print (fstL (L 1 2), sndL (L undefined 2))
  print (map (P 1) [2, 3])
  print (zipWith P [1, 2] [3, 4])
  print (apply2 P)
  print (toList (foldr insert Leaf [5, 2, 8, 1, 9, 3 :: Int]))
  print (sumP 1000 (P 0 0))
  print (shapeN (Shape "s" 3))
  print (gsnd (MkG 1 2), gsnd (MkH 3 4))
  print (fmap (+ 1) (F (1 :: Int) 2))
  let r = R { rx = 1, ry = undefined } :: Q Int
      r2 = r { rx = 5 }
      r3 = r { ry = 7 } :: Q Int
  print (rx r2, r3)
  let m1 = MA { mv = 1, mw = 2, pad = 0 }
      m2 = MB { mv = 3, mw = 4, pad = 0 }
      m3 = m2 { mv = undefined }
  print (mv (m1 { mv = 9 }), mw (m1 { mv = 9 }), mw m3)
  let w = Wide { w1 = undefined, w2 = 2, w3 = undefined, w4 = "x" }
  print (w2 w, w4 w, (w { w1 = 1, w3 = 3 }) == Wide 1 2 3 "x")
  let partial = P undefined
      unused = P undefined 1
      inList = [P 1 undefined, P 2 3]
  print (length inList, partial `seq` lazyOk 1)
  print (case Just (P 4 5) of
           Just (P a b) -> a + b
           Nothing -> 0)
