module Main where

-- G4: mutual class-constrained polymorphic recursion. `pp` recurses
-- through `qq` at [a], so neither can specialize; the cap trips on one
-- and the discovery walk drags the sibling into dictionary passing too
-- (rewrite_dict_expr's general_callee marking). `both` adds the
-- MULTI-CONSTRAINT fallback: two dictionaries built and passed in
-- declaration order through the same polymorphic recursion. `partial`
-- applies a dict-passing function partially OUTSIDE its own body at a
-- concrete type (the call-site saturation route).

pp :: Show a => Int -> a -> String
pp 0 x = show x
pp n x = qq (n - 1) [x]

qq :: Show a => Int -> a -> String
qq 0 x = show x
qq n x = pp (n - 1) [x]

both :: (Show a, Eq a) => Int -> a -> a -> String
both 0 x y = show (x == y)
both n x y = both (n - 1) [x] [y]

partial :: [Int] -> [String]
partial = map (pp 1)

main :: IO ()
main = do
  putStrLn (pp 3 (1 :: Int))
  putStrLn (qq 2 (2 :: Int))
  putStrLn (both 2 (3 :: Int) 3)
  putStrLn (both 3 (4 :: Int) 5)
  mapM_ putStrLn (partial [6, 7])

-- expect: [[[1]]]
-- expect: [[2]]
-- expect: True
-- expect: False
-- expect: [6]
-- expect: [7]
