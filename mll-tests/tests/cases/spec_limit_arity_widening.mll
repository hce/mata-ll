-- Regression (fresh-eyes review 2026-10, T11): a polymorphic function past
-- the specialization cap keeps a copy per arrow shape, so a use that widens
-- its arity still has code of its arity. Past the cap a constraint-free
-- function falls back to its one generic copy and a constrained one to its
-- one dictionary form, each emitted with the DECLARED type's arrow count of
-- parameters; a use that instantiates a result type variable to a function
-- type passes more arguments than that, and the body applies its
-- function-typed parameter with fewer: `apply f x = f x` at `b := Int -> Int`
-- called a two-parameter lambda with one argument ("attempt to perform
-- arithmetic on a nil value"). Every widening use now goes to a copy keyed by
-- the use's arrow shape (one per shape, not per type): an ordinary
-- specialization of a constraint-free function, a dictionary-passing copy of
-- a constrained one. Shapes: the reported one; widening by one and by two
-- arrows; a widening use before the trip and after it; the widening use
-- being the one that trips (`apply2`, used at exactly 17 types first: the
-- cap keeps 17 copies, the 18th distinct instantiation trips); a
-- first-class partial application;
-- widening through a point-free definition, a top-level helper and a
-- where-bound one; the constrained twin, used from main and from inside
-- another dictionary-form body (`describe`); a const shape whose FIRST
-- variable widens, unconstrained and with the other variable constrained;
-- and a constraint-free function past the cap calling a constrained one that
-- is past it too (`wrap`: its generic copy is an ordinary caller of the
-- dictionary form, not a dictionary-form body itself).

apply :: (a -> b) -> a -> b
apply f x = f x

apply2 :: (a -> b) -> a -> b
apply2 f x = f x

applyS :: Show a => (a -> b) -> a -> b
applyS f x = f x

k :: a -> b -> a
k x _ = x

kS :: Show b => a -> b -> a
kS x _ = x

wide :: Int -> Int -> Int
wide = apply (+)

helper :: Int -> Int -> Int
helper x y = apply (\a b -> a - b) x y

describe :: Show a => a -> String
describe x = applyS (\y n -> show y <> "/" <> show n) x (1 :: Int)

wrap :: a -> b -> String
wrap _ _ = describe (1 :: Int)

main :: IO ()
main = do
  print (apply (\x y -> x + y) (1 :: Int) 2)
  print (apply id (1 :: Int))
  print (apply id (2.5 :: Number))
  print (apply id "s")
  print (apply id True)
  print (apply id [1 :: Int])
  print (apply id (Just (1 :: Int)))
  print (apply id ((1 :: Int), (2 :: Int)))
  print (apply id ["a"])
  print (apply id (Just "b"))
  print (apply id ((1 :: Int), "c"))
  print (apply id [True])
  print (apply id (Just False))
  print (apply id (True, (3 :: Int)))
  print (apply id [[4 :: Int]])
  print (apply id (Just (Just (5 :: Int))))
  print (apply id ((6 :: Int), (7 :: Int), (8 :: Int)))
  print (apply id [9.5 :: Number])
  print (apply (\x y -> x * y) (3 :: Int) 4)
  print (apply (\x y z -> x + y + z) (1 :: Int) 2 3)
  print (map (\g -> g 10) (map (apply (+)) [1, 2, 3 :: Int]))
  print (zipWith (apply (+)) [1, 2 :: Int] [30, 40])
  print (wide 5 6)
  print (helper 20 5)
  print (local 2 3)
  print (apply2 id (1 :: Int))
  print (apply2 id (2.5 :: Number))
  print (apply2 id "s")
  print (apply2 id True)
  print (apply2 id [1 :: Int])
  print (apply2 id (Just (1 :: Int)))
  print (apply2 id ((1 :: Int), (2 :: Int)))
  print (apply2 id ["a"])
  print (apply2 id (Just "b"))
  print (apply2 id ((1 :: Int), "c"))
  print (apply2 id [True])
  print (apply2 id (Just False))
  print (apply2 id (True, (3 :: Int)))
  print (apply2 id [[4 :: Int]])
  print (apply2 id (Just (Just (5 :: Int))))
  print (apply2 id ((6 :: Int), (7 :: Int), (8 :: Int)))
  print (apply2 id [9.5 :: Number])
  print (apply2 (\x y -> x + y) (10 :: Int) 20)
  putStrLn (applyS show (1 :: Int))
  putStrLn (applyS show (2.5 :: Number))
  putStrLn (applyS show "s")
  putStrLn (applyS show True)
  putStrLn (applyS show [1 :: Int])
  putStrLn (applyS show (Just (1 :: Int)))
  putStrLn (applyS show ((1 :: Int), (2 :: Int)))
  putStrLn (applyS show ["a"])
  putStrLn (applyS show (Just "b"))
  putStrLn (applyS show ((1 :: Int), "c"))
  putStrLn (applyS show [True])
  putStrLn (applyS show (Just False))
  putStrLn (applyS show (True, (3 :: Int)))
  putStrLn (applyS show [[4 :: Int]])
  putStrLn (applyS show (Just (Just (5 :: Int))))
  putStrLn (applyS show ((6 :: Int), (7 :: Int), (8 :: Int)))
  putStrLn (applyS show [9.5 :: Number])
  print (applyS (\x y -> x + y) (1 :: Int) 2)
  print (applyS (\x y z -> x + y + z) (1 :: Int) 2 3)
  print (map (\g -> g 10) (map (applyS (+)) [1, 2, 3 :: Int]))
  putStrLn (describe (1 :: Int))
  putStrLn (describe (2.5 :: Number))
  putStrLn (describe "s")
  putStrLn (describe True)
  putStrLn (describe [1 :: Int])
  putStrLn (describe (Just (1 :: Int)))
  putStrLn (describe ((1 :: Int), (2 :: Int)))
  putStrLn (describe ["a"])
  putStrLn (describe (Just "b"))
  putStrLn (describe ((1 :: Int), "c"))
  putStrLn (describe [True])
  putStrLn (describe (Just False))
  putStrLn (describe (True, (3 :: Int)))
  putStrLn (describe [[4 :: Int]])
  putStrLn (describe (Just (Just (5 :: Int))))
  putStrLn (describe ((6 :: Int), (7 :: Int), (8 :: Int)))
  putStrLn (describe [9.5 :: Number])
  putStrLn (describe (Just [2 :: Int]))
  print (k (\y -> y + 1) "s" (5 :: Int))
  print (k (1 :: Int) ())
  print (k (2.5 :: Number) ())
  print (k "s" ())
  print (k True ())
  print (k [1 :: Int] ())
  print (k (Just (1 :: Int)) ())
  print (k ((1 :: Int), (2 :: Int)) ())
  print (k ["a"] ())
  print (k (Just "b") ())
  print (k ((1 :: Int), "c") ())
  print (k [True] ())
  print (k (Just False) ())
  print (k (True, (3 :: Int)) ())
  print (k [[4 :: Int]] ())
  print (k (Just (Just (5 :: Int))) ())
  print (k ((6 :: Int), (7 :: Int), (8 :: Int)) ())
  print (k [9.5 :: Number] ())
  print (k (+) True (1 :: Int) 2)
  putStrLn (kS "k" (1 :: Int))
  putStrLn (kS "k" (2.5 :: Number))
  putStrLn (kS "k" "s")
  putStrLn (kS "k" True)
  putStrLn (kS "k" [1 :: Int])
  putStrLn (kS "k" (Just (1 :: Int)))
  putStrLn (kS "k" ((1 :: Int), (2 :: Int)))
  putStrLn (kS "k" ["a"])
  putStrLn (kS "k" (Just "b"))
  putStrLn (kS "k" ((1 :: Int), "c"))
  putStrLn (kS "k" [True])
  putStrLn (kS "k" (Just False))
  putStrLn (kS "k" (True, (3 :: Int)))
  putStrLn (kS "k" [[4 :: Int]])
  putStrLn (kS "k" (Just (Just (5 :: Int))))
  putStrLn (kS "k" ((6 :: Int), (7 :: Int), (8 :: Int)))
  putStrLn (kS "k" [9.5 :: Number])
  print (kS (+) (Just True) (1 :: Int) 2)
  print (map (kS (\y -> y * 2) "t") [1, 2 :: Int])
  putStrLn (wrap (1 :: Int) ())
  putStrLn (wrap (2.5 :: Number) ())
  putStrLn (wrap "s" ())
  putStrLn (wrap True ())
  putStrLn (wrap [1 :: Int] ())
  putStrLn (wrap (Just (1 :: Int)) ())
  putStrLn (wrap ((1 :: Int), (2 :: Int)) ())
  putStrLn (wrap ["a"] ())
  putStrLn (wrap (Just "b") ())
  putStrLn (wrap ((1 :: Int), "c") ())
  putStrLn (wrap [True] ())
  putStrLn (wrap (Just False) ())
  putStrLn (wrap (True, (3 :: Int)) ())
  putStrLn (wrap [[4 :: Int]] ())
  putStrLn (wrap (Just (Just (5 :: Int))) ())
  putStrLn (wrap ((6 :: Int), (7 :: Int), (8 :: Int)) ())
  putStrLn (wrap [9.5 :: Number] ())
  putStrLn (wrap (3 :: Int) ())
  where
    local p q = apply (\a b c -> a * b + c) (p :: Int) q 1
