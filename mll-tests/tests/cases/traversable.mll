-- Traversable: traverse is a class method (instances: [], Maybe, Either);
-- sequenceA is generic over it. liftA2 (an Applicative method) is the
-- building block of the list instance, so it is exercised here too.

half :: Int -> Maybe Int
half n = if mod n 2 == 0 then Just (div n 2) else Nothing

main :: IO ()
main = do
    -- Maybe applicative over a list: one Nothing poisons the whole result
    assert (traverse half [2, 4, 6] == Just [1, 2, 3]) "traverse all even"
    assert (traverse half [2, 3, 6] == Nothing) "traverse blocks on odd"
    assert (traverse half ([] :: [Int]) == Just []) "traverse empty"
    -- sequenceA of a list of Maybes
    assert (sequenceA [Just 1, Just 2] == Just [1, 2]) "sequenceA all Just"
    assert (sequenceA [Just 1, Nothing] == Nothing) "sequenceA with Nothing"
    -- traverse over Maybe
    assert (traverse half (Just 4) == Just (Just 2)) "traverse Just even"
    assert (traverse half (Just 3) == Nothing) "traverse Just odd"
    assert (traverse half (Nothing :: Maybe Int) == Just Nothing) "traverse Nothing"
    -- traverse over Either (Right visited, Left passed through)
    case traverse half (Right 8 :: Either String Int) of
        Just (Right n) -> assert (n == 4) "traverse Right"
        _ -> error "traverse Right: wrong shape"
    case traverse half (Left "no" :: Either String Int) of
        Just (Left s) -> assert (s == "no") "traverse Left"
        _ -> error "traverse Left: wrong shape"
    -- the list applicative (nondeterminism)
    assert (traverse (\x -> [x, x + 10]) [1, 2] == [[1, 2], [1, 12], [11, 2], [11, 12]])
        "traverse list applicative"
    -- the IO applicative
    rs <- traverse (\x -> pure (x * 2)) [1, 2, 3]
    assert (rs == [2, 4, 6]) "traverse IO"
    r2 <- sequenceA [pure 1, pure 2]
    assert (r2 == [1, 2]) "sequenceA IO"
    r3 <- traverse (\x -> pure (x + 1)) (Just 41)
    assert (r3 == Just 42) "traverse Maybe with IO"
    -- liftA2 directly
    assert (liftA2 (\x y -> x + y) (Just 1) (Just 2) == Just 3) "liftA2 Maybe"
    assert (liftA2 (\x y -> x + y) (Just 1) (Nothing :: Maybe Int) == Nothing) "liftA2 Nothing"
    assert (liftA2 (\x y -> x + y) [1, 2] [10, 20] == [11, 21, 12, 22]) "liftA2 list"
    r4 <- liftA2 (\x y -> x * y) (pure 6) (pure 7)
    assert (r4 == 42) "liftA2 IO"
