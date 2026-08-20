-- As-patterns (Haskell 2010 §3.17, apat → var @ apat): the variable binds
-- the WHOLE value while the inner pattern destructures it. GHC clause-order
-- laziness must survive — `xs@p` forces exactly when `p` forces, and a
-- matching earlier clause never forces a later clause's as-pattern.

-- Top-level clauses: the whole value and its parts in one clause.
firstTwo :: [Int] -> ([Int], [Int])
firstTwo all@(x : y : _) = (all, [x, y])
firstTwo xs = (xs, xs)

-- An as-pattern over an irrefutable inner binds lazily, like a bare var.
lazyOuter :: [Int] -> Int
lazyOuter xs@_ = 0

-- Clause-order laziness: clause 0 matching must not force column 1, which
-- only clause 1 takes apart.
pick :: Int -> [Int] -> [Int]
pick 0 _ = []
pick n whole@(x : _) = x : whole
pick _ _ = [-1]

-- Nested inside a constructor argument: bind the pair AND its fields.
reuse :: Maybe (Int, Int) -> [(Int, Int)]
reuse (Just p@(a, b)) = [p, (b, a)]
reuse Nothing = []

-- In a case branch and a where-local clause.
sums :: [Int] -> (Int, Int)
sums ys = case ys of
    ws@(w : _) -> (w, whole ws)
    [] -> (0, 0)
  where
    whole zs@(z : rest) = z + length rest
    whole [] = 0

main :: IO ()
main = do
    assert (firstTwo [1, 2, 3] == ([1, 2, 3], [1, 2])) "as binds whole and parts"
    assert (firstTwo [9] == ([9], [9])) "fall-through clause"
    assert (lazyOuter (error "outer unforced") == 0) "as over _ stays lazy"
    assert (pick 0 (error "later-clause as unforced") == []) "clause order lazy"
    assert (pick 2 [7, 8] == [7, 7, 8]) "as after split"
    assert (reuse (Just (3, 4)) == [(3, 4), (4, 3)]) "nested as in Just"
    assert (reuse Nothing == []) "nothing arm"
    assert (sums [5, 6, 7] == (5, 7)) "case and where as-patterns"
    assert (sums [] == (0, 0)) "empty case arm"
    putStrLn "as-patterns ok"
-- expect: as-patterns ok
