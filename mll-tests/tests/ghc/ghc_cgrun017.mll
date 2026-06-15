-- GHC cgrun017: Maybe operations
-- Tests Maybe as a functor and in pattern matching

safeDiv :: Integer -> Integer -> Maybe Integer
safeDiv _ 0 = Nothing
safeDiv a b = Just (a `div` b)

safeHead :: [Integer] -> Maybe Integer
safeHead [] = Nothing
safeHead (x:_) = Just x

fromMaybe :: Integer -> Maybe Integer -> Integer
fromMaybe def Nothing  = def
fromMaybe _   (Just x) = x

main :: IO ()
main = do
    assert (safeDiv 10 2 == Just 5) "div ok"
    assert (safeDiv 10 0 == Nothing) "div zero"
    assert (safeHead [1, 2, 3] == Just 1) "head ok"
    assert (safeHead ([] :: [Integer]) == Nothing) "head empty"
    assert (fromMaybe 0 (Just 42) == 42) "fromMaybe Just"
    assert (fromMaybe 0 Nothing == 0) "fromMaybe Nothing"
    assert (fmap (* 2) (Just 5) == Just 10) "fmap Just"
    assert (fmap (* 2) (Nothing :: Maybe Integer) == Nothing) "fmap Nothing"
    putStrLn "ok"
