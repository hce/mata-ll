-- The standard containers idiom: the TYPE imported unqualified, the
-- operations through an alias.
--
--     import Data.Map (Map)
--     import qualified Data.Map as M
--
-- This was rejected outright ("importing 'Data.Map' unqualified conflicts
-- with 'null'"): the collision check ignored the import list, and the
-- names the list left out still flooded the flat namespace. Now only the
-- names an import form makes visible take part, the rest are renamed out
-- of the way, and a bare `filter`/`null`/`lookup` means the Prelude's.
import Data.Map (Map)
import qualified Data.Map as M

-- The unqualified type and the alias name the same type.
count :: Map String Int -> Int
count = M.size

bump :: M.Map String Int -> Map String Int
bump = M.map (+ 1)

main :: IO ()
main = do
    let m = M.fromList [("a", 1), ("b", 2)]
    assert (count m == 2) "Map in a signature is M.Map"
    assert (M.toList (bump m) == [("a", 2), ("b", 3)]) "alias operations"
    assert (M.lookup "a" m == Just 1) "M.lookup is Data.Map's"
    assert (lookup "a" [("a", 5)] == Just 5) "bare lookup is the Prelude's"
    assert (filter even [1, 2, 3, 4] == [2, 4]) "bare filter is the Prelude's"
    assert (null ([] :: [Int])) "bare null is the Prelude's"
    assert (not (M.null m)) "M.null is Data.Map's"
    putStrLn "data map idiom ok"
