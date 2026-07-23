-- Data.Foldable / Data.Traversable: GHC-style imports resolve, toList
-- (which lives here, not in the Prelude) works, and the re-exported
-- Prelude generics stay usable through the module.

import Data.Foldable (toList, foldl', find)
import Data.Traversable (traverse, sequenceA)

main :: IO ()
main = do
    assert (toList (Just 5) == [5]) "toList Just"
    assert (toList (Nothing :: Maybe Int) == []) "toList Nothing"
    assert (toList [1, 2, 3] == [1, 2, 3]) "toList list"
    assert (toList (Right 9 :: Either String Int) == [9]) "toList Right"
    assert (toList (Left "e" :: Either String Int) == []) "toList Left"
    -- toList streams lazily: it can take from an infinite structure
    assert (take 3 (toList (iterate (\x -> x * 2) 1)) == [1, 2, 4]) "toList infinite"
    -- re-exports picked up through Data.Foldable
    assert (foldl' (\acc x -> acc + x) 0 [1, 2, 3] == 6) "foldl' re-export"
    assert (find (\x -> x > 1) [1, 2, 3] == Just 2) "find re-export"
    -- re-exports picked up through Data.Traversable
    assert (sequenceA [Just 1, Just 2] == Just [1, 2]) "sequenceA re-export"
    assert (traverse (\x -> Just x) [1, 2] == Just [1, 2]) "traverse re-export"
