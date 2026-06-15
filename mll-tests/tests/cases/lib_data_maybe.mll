-- Tests for Data.Maybe module
import Data.Maybe

main :: IO ()
main = do
    assert (isJust (Just 1) == True) "isJust Just"
    assert (isJust Nothing == False) "isJust Nothing"
    assert (isNothing Nothing == True) "isNothing Nothing"
    assert (isNothing (Just 1) == False) "isNothing Just"
    assert (fromJust (Just 42) == 42) "fromJust"
    assert (fromMaybe 0 Nothing == 0) "fromMaybe Nothing"
    assert (fromMaybe 0 (Just 42) == 42) "fromMaybe Just"
    assert (maybe 0 (\x -> x + 1) (Just 5) == 6) "maybe Just"
    assert (maybe 0 (\x -> x + 1) Nothing == 0) "maybe Nothing"
    assert (catMaybes [Just 1, Nothing, Just 3] == [1, 3]) "catMaybes"
    assert (mapMaybe (\x -> if x > 2 then Just (x * 10) else Nothing) [1, 2, 3, 4] == [30, 40]) "mapMaybe"
    assert (listToMaybe [1, 2, 3] == Just 1) "listToMaybe"
    assert (listToMaybe ([] :: [Integer]) == Nothing) "listToMaybe empty"
    assert (maybeToList (Just 1) == [1]) "maybeToList Just"
    assert (maybeToList (Nothing :: Maybe Integer) == []) "maybeToList Nothing"
