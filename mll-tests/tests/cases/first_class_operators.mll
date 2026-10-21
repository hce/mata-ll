module Main where

-- M5/T8 (2026-10 fresh-eyes review): a first-class operator `(op)` is the
-- value of whatever `op` denotes. A user operator, a Prelude source
-- operator (`^`, `$!`, `=<<`, `<$`) and an imported operator are the
-- function itself, so they specialize, widen their arity and take the
-- dictionary fallback exactly as the prefix spelling does; a class-method
-- operator is dispatched like an infix use; the operators codegen lowers
-- itself (`++`, `!!`, `:`, `$`, `.`, `>>=`, `>>`, the Lua-native set) keep
-- their lowering, and each returns a value in WHNF (`foldr (++) []` with an
-- empty first list returned the suspended next step itself).

import OpsExports ((|>), (&))

infixr 5 <+>
(<+>) :: Int -> Int -> Int
a <+> b = a * 10 + b

(<.>) :: (a -> b) -> (b -> c) -> a -> c
f <.> g = \x -> g (f x)

(<#>) :: Show a => a -> a -> String
a <#> b = show a <> "/" <> show b

class Joinable a where
    (<&&>) :: a -> a -> a

instance Joinable Int where
    a <&&> b = a + b

instance Joinable [a] where
    a <&&> b = a ++ b

data Op = Op (Int -> Int -> Int)

runOp :: Op -> Int -> Int -> Int
runOp (Op f) a b = f a b

pick :: Bool -> Int -> Int -> Int
pick True = (<+>)
pick False = (-)

combine :: Int -> Int -> Int
combine a b = a + b + 1

sections :: [Int] -> [Int]
sections xs = map (`mul1` 2) xs
  where
    mul1 a b = a * b + 1

data NumBox = forall a. (Num a, Show a) => NumBox a

square :: NumBox -> String
square (NumBox x) = show ((^) x (2 :: Int))

main :: IO ()
main = do
    print ((^) 3 4 :: Int)
    print (map ((^) 2) [0, 1, 5] :: [Int])
    print (zipWith (^) [2, 3 :: Int] [10, 3])
    print (foldr (^) 1 [2, 3 :: Int])
    print (zipWith (^) [1.5, 2.0 :: Number] [2, 10])
    print ((^) 1.5 3 :: Number)
    print ((^) 2 100 :: Integer)
    print (foldl (^) (2 :: Integer) [10, 2 :: Int])
    print (zipWith (^) [10, 3 :: Integer] [30, 40])
    print (map (2 ^) [3, 4 :: Int])
    print (map (^ 2) [3, 4 :: Integer])
    print (foldr (++) [] [[1 :: Int], []])
    print (foldr (++) [] [[], [2 :: Int], [], [3]])
    print (foldr (++) [] [[[1 :: Int]], [], [[2], []]])
    print (foldl (++) [] [[1 :: Int], [], [2]])
    print (zipWith (<>) ["a", ""] ["", "b"])
    print (foldr (<>) "" ["a", "b", "c"])
    print (zipWith (==) [[1 :: Int], [2]] [[1], [3]])
    print (zipWith (/=) [Just (1 :: Int)] [Nothing])
    print (zipWith (<) [(1 :: Int, 2 :: Int)] [(1, 3)])
    print (zipWith (:) [1 :: Int, 2] [[3], [4]])
    print (foldr (:) [] [1 :: Int, 2, 3])
    print (zipWith (!!) [[1 :: Int, 2], [3, 4]] [1, 0])
    print (map ((!!) [5 :: Int, 6, 7]) [0, 2])
    print (zipWith ($) [succ, pred] [1 :: Int, 2])
    print (foldr ($) 0 [succ, (* 2)] :: Int)
    print (foldr (.) id [succ, (* 2)] (3 :: Int))
    print (map ($ 3) (zipWith (.) [succ, (* 2)] [pred, succ]) :: [Int])
    print (foldl (>>=) (Just 1) [\x -> Just (x + 1), \x -> Just (x * 2 :: Int)])
    print (foldl (>>=) [1 :: Int, 2] [\x -> [x, x * 10]])
    r <- foldl (>>=) (return 1) [\x -> print x >> return (x + 1), \x -> return (x * 3 :: Int)]
    print r
    print (foldr (>>) (Just 0) [Just 1, Nothing :: Maybe Int])
    print (foldr (>>) [0 :: Int] [[1, 2]])
    foldr (>>) (return ()) [print (1 :: Int), print (2 :: Int)]
    print ((=<<) (\x -> [x, x]) [1 :: Int, 2])
    print ((=<<) (\x -> Just (x + 1)) (Just (1 :: Int)))
    print (foldr (=<<) (Just 1) [\x -> Just (x + 1), \x -> Just (x * 2 :: Int)])
    print (($!) succ (1 :: Int))
    print (zipWith ($!) [succ, pred] [1 :: Int, 2])
    print ((<$) (0 :: Int) [1 :: Int, 2, 3])
    print (zipWith (<$) [7 :: Int, 8] [Just (1 :: Int), Nothing])
    print (zipWith (<$>) [succ, pred] [[1 :: Int, 2], [3]])
    print (foldr (<$>) (Just (1 :: Int)) [succ, (* 2)])
    print (zipWith (<*>) [[succ, pred]] [[1 :: Int, 2]])
    print (foldr (<*>) (Just (1 :: Int)) [Just succ])
    print (foldr (<+>) 0 [1, 2, 3])
    print (zipWith (<+>) [1, 2] [3, 4])
    print (map ((<+>) 1) [2, 3])
    print (map (<+> 1) [2, 3])
    print (map (1 <+>) [2, 3])
    print (map (`combine` 1) [2, 3])
    print (map (1 `combine`) [2, 3])
    print (sections [2, 3])
    print (runOp (Op (<+>)) 4 5)
    print (pick True 6 7, pick False 6 7)
    print (foldr (<&&>) (0 :: Int) [1, 2, 3])
    print (foldr (<&&>) [] [[1 :: Int], [2, 3]])
    print (zipWith3 (<.>) [succ] [(* 2)] [3 :: Int])
    print (map ((<.>) succ (* 2)) [1, 2 :: Int])
    print (foldr (|>) 0 [1, 2, 3])
    print (zipWith (&) [1 :: Int, 2] [succ, pred])
    print (map square [NumBox (3 :: Int), NumBox (1.5 :: Number), NumBox (10 :: Integer)])
    print (zipWith (<#>) [1 :: Int] [2])
    print (zipWith (<#>) [1.5 :: Number] [2])
    print (zipWith (<#>) [1 :: Integer] [2])
    print (zipWith (<#>) [True] [False])
    print (zipWith (<#>) ["a"] ["b"])
    print (zipWith (<#>) [[1 :: Int]] [[2]])
    print (zipWith (<#>) [[[1 :: Int]]] [[[2]]])
    print (zipWith (<#>) [Just (1 :: Int)] [Nothing])
    print (zipWith (<#>) [Just True] [Just False])
    print (zipWith (<#>) [(1 :: Int, True)] [(2, False)])
    print (zipWith (<#>) [Left 1 :: Either Int Bool] [Right True])
    print (zipWith (<#>) [[Just (1 :: Int)]] [[Nothing]])
    print (zipWith (<#>) [Just [1 :: Int]] [Nothing])
    print (zipWith (<#>) [(True, "x")] [(False, "y")])
    print (zipWith (<#>) [[True]] [[False]])
    print (zipWith (<#>) [Just (1.5 :: Number)] [Nothing])
    print (zipWith (<#>) [(1 :: Int, 2 :: Int, 3 :: Int)] [(4, 5, 6)])
    print (zipWith (<#>) [Just (Just (1 :: Int))] [Just Nothing])
    print (zipWith (<#>) [[1.5 :: Number]] [[2.5]])
    print (zipWith (<#>) [Right "c" :: Either Int String] [Left 1])
    print (foldr (<#>) "z" ["a", "b"])
