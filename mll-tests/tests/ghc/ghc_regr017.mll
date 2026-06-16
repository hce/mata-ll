-- ghc_regr017: Monomorphization: same polymorphic function at 3+ types

-- Polymorphic identity
myId :: a -> a
myId x = x

-- Polymorphic flip
myFlip :: (a -> b -> c) -> b -> a -> c
myFlip f y x = f x y

-- Polymorphic const
myConst :: a -> b -> a
myConst x _ = x

-- Polymorphic pair maker
makePair :: a -> b -> (a, b)
makePair x y = (x, y)

-- Polymorphic maybe fold
foldMaybe :: b -> (a -> b) -> Maybe a -> b
foldMaybe def _ Nothing  = def
foldMaybe _   f (Just x) = f x

-- Polymorphic list map (manual)
myMap :: (a -> b) -> [a] -> [b]
myMap _ []     = []
myMap f (x:xs) = f x : myMap f xs

-- Polymorphic length
myLength :: [a] -> Integer
myLength []     = 0
myLength (_:xs) = 1 + myLength xs

main :: IO ()
main = do
    -- myId at Integer, String, Bool, [Integer]
    assert (myId (42 :: Integer) == 42) "id Integer"
    assert (myId "hello" == "hello") "id String"
    assert (myId True == True) "id Bool"
    assert (myId [1, 2, 3 :: Integer] == [1, 2, 3]) "id list"
    assert (myId (Just (5 :: Integer)) == Just 5) "id Maybe"

    -- myFlip at multiple types
    assert (myFlip (-) 3 10 == 7) "flip Integer"
    assert (myFlip (\a b -> a <> b) "world" "hello" == "helloworld") "flip String"
    assert (myFlip myConst "ignored" True == True) "flip const"

    -- myConst at multiple types
    assert (myConst (1 :: Integer) "x" == 1) "const Int"
    assert (myConst "kept" (99 :: Integer) == "kept") "const String"
    assert (myConst True False == True) "const Bool"

    -- makePair at multiple type combinations
    assert (makePair (1 :: Integer) "a" == (1, "a")) "pair Int String"
    assert (makePair True (3.14 :: Number) == (True, 3.14)) "pair Bool Number"
    assert (makePair "x" "y" == ("x", "y")) "pair String String"

    -- foldMaybe at multiple types
    assert (foldMaybe 0 (\n -> n + 1) (Just (5 :: Integer)) == 6) "foldMaybe Just Int"
    assert (foldMaybe 0 (\n -> n + 1) (Nothing :: Maybe Integer) == 0) "foldMaybe Nothing Int"
    assert (foldMaybe "no" (\s -> s <> "!") (Just "hi") == "hi!") "foldMaybe Just String"
    assert (foldMaybe False not (Just True) == False) "foldMaybe Just Bool"

    -- myMap and myLength at different element types
    assert (myMap (\n -> n * 2) [1, 2, 3 :: Integer] == [2, 4, 6]) "map Int"
    assert (myMap (\s -> s <> "!") ["a", "b"] == ["a!", "b!"]) "map String"
    assert (myMap not [True, False, True] == [False, True, False]) "map Bool"
    assert (myLength [1, 2, 3 :: Integer] == 3) "length Int"
    assert (myLength ["a", "b"] == 2) "length String"
    assert (myLength ([] :: [Bool]) == 0) "length empty"

    putStrLn "ok"
