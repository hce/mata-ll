-- The Semigroup/Monoid instances for String and [a] are ordinary source
-- instances in lib/Prelude.mll (moved out of the compiler's Rust tables).
-- This exercises them over CONSTRUCTED values — strings and lists built up
-- by recursion, not written as literals — so a marshalling or dispatch bug
-- in the moved instances cannot hide behind a constant-folded literal. It
-- also pins mempty at both element types and the polymorphic foldMap path.

-- Build a String by mappend-folding a list of pieces (uses mempty as the seed
-- and mappend to combine — the String Monoid, over non-literal inputs).
concatStrings :: [String] -> String
concatStrings []     = mempty
concatStrings (x:xs) = mappend x (concatStrings xs)

-- Build a list the same way (the [a] Monoid).
concatLists :: [[a]] -> [a]
concatLists []       = mempty
concatLists (xs:xss) = mappend xs (concatLists xss)

-- A polymorphic Monoid-constrained function (not a builtin): must resolve
-- through the moved instances at both String and [a].
mconcat' :: Monoid m => [m] -> m
mconcat' []     = mempty
mconcat' (x:xs) = mappend x (mconcat' xs)

main :: IO ()
main = do
    -- Strings assembled from show results, not literals.
    let pieces = map show [10, 20, 30]
    assert (concatStrings pieces == "102030") "mappend over constructed strings"
    assert (mconcat' pieces == "102030") "polymorphic mconcat' at String"

    -- <> on constructed strings.
    assert ((show 1 <> show 2 <> show 3) == "123") "constructed string <>"
    assert ((mempty <> concatStrings pieces) == "102030") "mempty is left identity (String)"
    assert ((concatStrings pieces <> mempty) == "102030") "mempty is right identity (String)"

    -- Lists assembled by mapping, then mappend-folded.
    let rows = map (\n -> [n, n + 1]) [1, 4, 7]
    assert (concatLists rows == [1, 2, 4, 5, 7, 8]) "mappend over constructed lists"
    assert (mconcat' rows == [1, 2, 4, 5, 7, 8]) "polymorphic mconcat' at [a]"
    assert (mappend mempty (concatLists rows) == [1, 2, 4, 5, 7, 8]) "mempty is left identity (list)"

    -- mempty determined by annotation at each element type.
    assert ((mempty :: String) == "") "mempty at String"
    assert ((mempty :: [Integer]) == []) "mempty at [Integer]"

    -- foldMap routes through the moved Monoid instances for both targets.
    assert (foldMap show [1, 2, 3] == "123") "foldMap into String monoid"
    assert (foldMap (\n -> [n, n]) [5, 6] == [5, 5, 6, 6]) "foldMap into list monoid"

    putStrLn "all monoid-instance tests passed"
-- expect: all monoid-instance tests passed
