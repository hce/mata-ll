module Main where

-- A use of a constrained polymorphic function whose use type keeps a
-- DEAD type variable — `b` below: unconstrained, never reaching the
-- body — used to be treated as "cannot specialize". Outside any
-- specialization of `f` it was pointed at the lexically-smallest
-- SIBLING specialization, whatever its type: `f (1 :: Int) []` ran the
-- Number copy (printing 1.0) or the String copy (crashing on a number),
-- and with no sibling it stayed on the generic copy, whose type-erased
-- show printed `T 1` as `(1)`. A dead variable cannot change the body,
-- so such a use now specializes at the canonicalized use type exactly
-- as a concrete use does.

data T = T Int deriving Show

data W = W Int

f :: Show a => a -> b -> String
f x _ = show x

-- The same use inside an instance method body …
instance Show W where
    show (W n) = "W:" <> f n []

-- … and inside a where-bound helper.
h :: Int -> String
h n = helper n
  where helper k = f k []

-- Polymorphic recursion in the dead variable alone: each level nests
-- the dead slot one list deeper, so the copies ladder up to the
-- specialization cap and the function goes over to dictionary passing —
-- exactly as the same ladder at a concrete `[Bool]`, `[[Bool]]`, … does.
deep :: Show a => Int -> a -> b -> String
deep 0 x _ = show x
deep n x y = deep (n - 1) x [y]

main :: IO ()
main = do
    -- a String and a Number sibling exist before the dead-variable uses
    putStrLn (f "s" True)
    putStrLn (f (1.5 :: Number) True)
    putStrLn (f (1 :: Int) [])
    putStrLn (f True [])
    putStrLn (f (7 :: Int) True)
    assert (f (1 :: Int) [] == "1") "an Int use with a dead list variable shows at Int"
    -- a derived Show through the dead-variable use
    putStrLn (f (T 1) [])
    putStrLn (f (Just (T 2)) [])
    assert (f (T 1) [] == "T 1") "a derived Show reaches the specialized copy"
    -- the where-bound helper, the instance method body, a partial
    -- application, a first-class value
    putStrLn (h 5)
    print (W 3)
    print (map (f (1 :: Int)) [[], []])
    let p = f True
    putStrLn (p [])
    assert (p [] == "True") "a first-class dead-variable use shows at Bool"
    putStrLn (deep 3 (1 :: Int) [])
    putStrLn (deep 3 "s" True)
