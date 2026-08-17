-- Test: a WHERE-BOUND multi-clause function forces an argument at entry
-- only when its FIRST clause scrutinizes it — GHC's top-to-bottom,
-- left-to-right laziness, the rule the top-level emitter already applies
-- (`zip [] undefined == []`). The where-group emitter forced a parameter
-- if ANY clause scrutinized it, so the local `go [] _ = []` below raised
-- on the bottom second argument instead of returning [].

zipLocal :: [Int] -> [Int] -> [(Int, Int)]
zipLocal xs ys = go xs ys
  where
    go [] _ = []
    go (a:as) (b:bs) = (a, b) : go as bs
    go _ [] = []

-- The mirror: when the FIRST clause scrutinizes the argument the entry
-- force is right, and a later clause's own scrutiny still happens.
firstOr :: [Int] -> Int -> Int
firstOr xs d = pick xs d
  where
    pick (x:_) _ = x
    pick [] 0 = 100
    pick [] n = n

main :: IO ()
main = do
    assert (length (zipLocal [] (error "second argument forced")) == 0)
        "where-local zip [] _ leaves the second argument unforced"
    assert (zipLocal [1, 2] [3, 4] == [(1, 3), (2, 4)]) "where-local zip still zips"
    assert (zipLocal [1] [] == []) "where-local zip: later clause matches"
    assert (firstOr [7] (error "default forced") == 7) "first-clause match leaves the default unforced"
    assert (firstOr [] 0 == 100 && firstOr [] 5 == 5) "later clauses scrutinize the default"
