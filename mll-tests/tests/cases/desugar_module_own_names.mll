import qualified DesugarOwn as D

main :: IO ()
main = do
    assert (D.pairsUp [1, 2, 3] == [10, 20, 30]) "a comprehension in a module with its own concatMap"
    assert (D.concatMap (\x -> [x, x]) [1, 2, 3] == [3, 3, 2, 2, 1, 1]) "that module's concatMap, by name"
    assert (D.upTo 4 == [1, 2, 3, 4]) "a range in a module with its own enumFromTo"
    assert (D.enumFromTo 1 4 == [4, 1]) "that module's enumFromTo, by name"
    assert (D.firstOf [7, 8] == 7) "a pattern lambda in a module with its own error"
    assert (D.error "unused" == 0 - 1) "that module's error, by name"
    assert (map D.band [5, 12, 13] == ["digit", "even", "other"]) "guard commas in a module with its own (&&)"
    t <- D.total [Just 1, Just 2]
    assert (t == 3) "a refutable bind in a module with its own error"
    print (D.pairsUp [1, 2, 3], D.concatMap (\x -> [x, x]) [1, 2, 3])
    print (D.upTo 4, D.enumFromTo 1 4)
    print (D.firstOf [7, 8], D.error "unused", t)
    print (map D.band [5, 12, 13])
