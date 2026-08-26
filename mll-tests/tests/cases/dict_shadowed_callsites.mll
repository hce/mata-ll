module Main where

-- G1: locals must shadow dict-passing globals in the dictionary rewrite
-- passes. Both functions trip the specialization cap (class-constrained
-- polymorphic recursion), so every later use of their NAMES goes through
-- rewrite_dict_call_sites — which used to rewrite shadowing locals into
-- DictCalls on the globals. `f` doubles as the Prelude-collision probe:
-- Prelude parameters named `f` (flip, concatMap, ...) used to produce a
-- spurious "cannot pass 'f' as a function value" cascade.

gg :: Show a => Int -> a -> String
gg 0 x = show x
gg n x = gg (n - 1) [x]

f :: Show a => Int -> a -> String
f 0 x = show x
f n x = f (n - 1) [x]

-- Clause parameter shadows the dict-passing global.
apply :: (Int -> Int -> String) -> String
apply gg = gg 1 2

-- Lambda parameter shadows it.
applyLam :: String
applyLam = (\gg -> gg 3 4) (\a b -> "lam:" <> show (a * b))

-- Let binding shadows it.
applyLet :: String
applyLet = let gg = \a b -> "let:" <> show (a - b) in gg 9 2

-- Case pattern variable shadows it.
applyCase :: Maybe (Int -> Int -> String) -> String
applyCase m = case m of
  Just gg -> gg 2 3
  Nothing -> "none"

-- Where-binding parameter shadows it (as a plain value).
applyWhere :: Int -> String
applyWhere n = go n
  where go gg = "where:" <> show (gg :: Int)

main :: IO ()
main = do
  putStrLn (gg 2 (7 :: Int))
  putStrLn (f 3 (1 :: Int))
  putStrLn (apply (\a b -> "param:" <> show (a + b)))
  putStrLn applyLam
  putStrLn applyLet
  putStrLn (applyCase (Just (\a b -> "case:" <> show (a + b))))
  putStrLn (applyWhere 5)

-- expect: [[7]]
-- expect: [[[1]]]
-- expect: param:3
-- expect: lam:12
-- expect: let:7
-- expect: case:5
-- expect: where:5
