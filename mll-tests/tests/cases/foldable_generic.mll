-- A18: the last list-only stragglers are Foldable/Traversable-generic, as
-- GHC's: and/or/any/all/concat/concatMap over Foldable (explicit lambdas
-- keep the accumulator lazy, so short-circuiting over infinite lists
-- survives the class foldr), mapM = traverse and sequence = sequenceA at
-- Monad, mapM_/sequence_/forM_ over Foldable, forM over Traversable.

main :: IO ()
main = do
    print (any even (Just 4))
    print (all even (Just 3))
    print (and (Just True))
    print (or (Nothing :: Maybe Bool))
    print (concat (Just [1, 2]))
    print (concatMap (\x -> [x, x]) (Just 5))
    print (any even (Right 2 :: Either String Int))
    print (all odd (Left "e" :: Either String Int))
    -- infinite lists: the short-circuit is load-tested, not assumed
    print (any (\x -> x > 3) [1 ..])
    print (or (map (\x -> x > 2) [1 ..]))
    -- the mapM family at Maybe and at lists
    r <- mapM (\x -> pure (x + 1)) (Just 4)
    print r
    mapM_ print (Just 7)
    s <- sequence (Just (pure 9))
    print (s :: Maybe Int)
    n <- mapM (\x -> pure (x * 2)) [1, 2, 3]
    print n
    q <- sequence [pure 1, pure 2]
    print q
