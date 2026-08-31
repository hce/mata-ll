-- A16: structural Ord for the compiler-owned shapes — lists, tuples, Maybe
-- — mirroring the structural Eq/Show machinery (typechecker gate in
-- structural_container_class; mono generates ord_compare_ per shape and
-- derives </<=/>/>=/max/min from it; runtime __mll_list_cmp/__mll_maybe_cmp
-- walk like their eq twins). Plus sort/sortBy (stable bottom-up mergesort),
-- which did not exist at all.

main :: IO ()
main = do
    -- lexicographic tuples, nested shapes, GHC's derived Maybe order
    print (sort [(2, 1), (1, 9), (1, 2), (2, 0)])
    print (sort [[3], [], [1, 2], [1]])
    print (sort [Just 2, Nothing, Just 1])
    print (sort [(1, Just 2), (1, Nothing), (0, Just 9)])
    print (compare (1, "b") (1, "a"))
    print (compare [True] [True, False])
    -- operators and selectors at structural types
    print ((1, 2) < (1, 3))
    print ([2, 1] >= [2])
    print (max (Just 1) Nothing)
    print (min [2] [1, 5])
    print (maximum [(1, 2), (3, 0), (2, 9)])
    print (minimum [Just 3, Just 1])
    -- stability: equal keys keep their source order under sortBy
    print (sortBy (\a b -> compare (fst a) (fst b)) [(1, "x"), (0, "z"), (1, "y")])
