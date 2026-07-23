-- Tests for Data.Map, imported `qualified` (the idiomatic way).
-- Data.Map defines map/filter/null/lookup, which collide with the Prelude;
-- qualification is what lets both coexist. Exercises:
--   * qualified value use-sites (M.insert, M.empty, ...)
--   * a qualified type in a signature (M.Map String Int)
--   * the colliding names resolving to the Data.Map versions
--   * internal cross-references inside Data.Map (map -> fromList/toList, etc.)
import qualified Data.Map as M

-- Qualified type in a signature; also forces Data.Map's `size` (not Prelude's).
count :: M.Map String Int -> Int
count = M.size

main :: IO ()
main = do
    let m = M.insert "a" 1 (M.insert "b" 2 (M.insert "c" 3 M.empty))
    assert (count m == 3) "qualified type sig + size"
    assert (M.member "a" m) "member present"
    assert (not (M.member "z" m)) "member absent"
    assert (M.lookup "b" m == Just 2) "lookup hit"
    assert (M.lookup "z" m == Nothing) "lookup miss"
    -- map/filter/null all collide with Prelude names — must hit Data.Map's.
    let m2 = M.map (\v -> v * 10) m
    assert (M.lookup "c" m2 == Just 30) "map values"
    let m3 = M.filter (\v -> v > 15) m2
    assert (M.size m3 == 2) "filter kept the big ones"
    assert (M.toList m3 == [("b", 20), ("c", 30)]) "toList sorted by key"
    assert (not (M.null m3)) "non-empty is not null"
    assert (M.null M.empty) "empty is null"
    -- union / fromList / keys / values
    let u = M.union m (M.fromList [("d", 4), ("a", 99)])
    assert (M.size u == 4) "union size"
    assert (M.lookup "a" u == Just 1) "union is left-biased"
    assert (M.keys m == ["a", "b", "c"]) "keys sorted"
    assert (M.values m == [1, 2, 3]) "values by sorted key"
    -- Prelude's colliding names still work unqualified in the same module.
    assert (map (\x -> x + 1) [1, 2, 3] == [2, 3, 4]) "Prelude map still works"
    assert (null ([] :: [Int])) "Prelude null still works"
    assert (filter (\x -> x > 1) [1, 2, 3] == [2, 3]) "Prelude filter still works"
