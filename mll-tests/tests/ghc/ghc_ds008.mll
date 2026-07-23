-- GHC ds008: Pattern matching on nested constructors
-- Just (Left x), Right (Just y), Nothing, etc.

unwrapJustLeft :: Maybe (Either Int String) -> Int
unwrapJustLeft (Just (Left n)) = n
unwrapJustLeft _               = 0 - 1

unwrapJustRight :: Maybe (Either Int String) -> String
unwrapJustRight (Just (Right s)) = s
unwrapJustRight _                = "default"

-- Deeply nested
data Wrapper a = W (Maybe a)
    deriving (Show, Eq)

fromMaybeInt :: Int -> Maybe Int -> Int
fromMaybeInt def m = case m of
    Nothing -> def
    Just n  -> n

peekWrapper :: Wrapper (Maybe Int) -> Int
peekWrapper (W outerMx) = case outerMx of
    Nothing    -> 0 - 1
    Just inner -> fromMaybeInt 0 inner

-- List of Maybes: pattern match generator with manual map
catMaybes :: [Maybe a] -> [a]
catMaybes []             = []
catMaybes (Nothing : xs) = catMaybes xs
catMaybes (Just x  : xs) = x : catMaybes xs

-- Either helpers
partitionEithers :: [Either a b] -> ([a], [b])
partitionEithers []           = ([], [])
partitionEithers (Left a : rest) =
    let p = partitionEithers rest
    in (a : fst p, snd p)
partitionEithers (Right b : rest) =
    let p = partitionEithers rest
    in (fst p, b : snd p)

main :: IO ()
main = do
    assert (unwrapJustLeft (Just (Left 42)) == 42)    "just left"
    assert (unwrapJustLeft (Just (Right "x")) == 0 - 1) "just right->default"
    assert (unwrapJustLeft Nothing == 0 - 1)            "nothing->default"

    assert (unwrapJustRight (Just (Right "hi")) == "hi") "just right"
    assert (unwrapJustRight (Just (Left 9))    == "default") "just left->default"

    assert (peekWrapper (W (Just (Just 7))) == 7)  "deep just just"
    -- W (Just Nothing): fromMaybeInt default when inner is Nothing
    let innerPeek = fromMaybeInt 0 (Nothing :: Maybe Int)
    assert (innerPeek == 0) "inner nothing gives 0"
    assert (peekWrapper (W (Nothing :: Maybe (Maybe Int))) == 0 - 1) "deep nothing"

    let ms = [Just 1, Nothing, Just 2, Nothing, Just 3]
    assert (catMaybes ms == [1, 2, 3]) "catMaybes"
    assert (catMaybes ([] :: [Maybe Int]) == []) "catMaybes empty"

    let es = [Left 1, Right "a", Left 2, Right "b"]
    let parts = partitionEithers es
    assert (fst parts == [1, 2]) "partition lefts"
    assert (snd parts == ["a", "b"]) "partition rights"

    putStrLn "ok"
