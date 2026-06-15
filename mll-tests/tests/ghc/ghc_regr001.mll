-- ghc_regr001: Deeply nested data constructor pattern matching (3+ levels)

data Color = Red | Green | Blue
    deriving (Show, Eq)

data Shape = Circle Number | Rect Number Number
    deriving (Show, Eq)

data Tagged a = Tag String a
    deriving (Show, Eq)

data Wrapper a = Wrap (Maybe (Tagged a))
    deriving (Show, Eq)

colorTag :: Color -> String -> String
colorTag c s
    | c == Red   = "red-" ++ s
    | c == Green = "green-" ++ s
    | otherwise  = "blue-" ++ s

-- 3-level deep pattern match: Wrap -> Just -> Tag -> Color
unwrap :: Wrapper Color -> String
unwrap (Wrap mx) = case mx of
    Nothing       -> "empty"
    Just (Tag s c) -> colorTag c s

-- 3-level: nested constructors in function args using guards in top-level pattern
describeShapeTag :: Tagged Shape -> String
describeShapeTag (Tag lbl (Circle r))
    | r > 5.0   = lbl ++ ":big-circle"
    | otherwise = lbl ++ ":small-circle"
describeShapeTag (Tag lbl (Rect w h))
    | w == h    = lbl ++ ":square"
    | otherwise = lbl ++ ":rect"

-- 4-level: Maybe (Either (Tagged Color))
data Nested = MkNested (Maybe (Either (Tagged Color) Integer))
    deriving (Show, Eq)

nestedColorTag :: Color -> String -> String
nestedColorTag c s
    | c == Red  = "left-red:" ++ s
    | otherwise = "left-other:" ++ s

extractNested :: Nested -> String
extractNested (MkNested mx) = case mx of
    Nothing        -> "nothing"
    Just (Right n) -> "int:" ++ show n
    Just (Left (Tag s c)) -> nestedColorTag c s

main :: IO ()
main = do
    assert (unwrap (Wrap Nothing) == "empty") "wrap nothing"
    assert (unwrap (Wrap (Just (Tag "x" Red))) == "red-x") "wrap red"
    assert (unwrap (Wrap (Just (Tag "y" Blue))) == "blue-y") "wrap blue"

    assert (describeShapeTag (Tag "s" (Circle 3.0)) == "s:small-circle") "small circle"
    assert (describeShapeTag (Tag "s" (Circle 10.0)) == "s:big-circle") "big circle"
    assert (describeShapeTag (Tag "s" (Rect 4.0 4.0)) == "s:square") "square"
    assert (describeShapeTag (Tag "s" (Rect 3.0 5.0)) == "s:rect") "rect"

    assert (extractNested (MkNested Nothing) == "nothing") "nested nothing"
    assert (extractNested (MkNested (Just (Right 42))) == "int:42") "nested int"
    assert (extractNested (MkNested (Just (Left (Tag "a" Red)))) == "left-red:a") "nested left-red"
    assert (extractNested (MkNested (Just (Left (Tag "b" Green)))) == "left-other:b") "nested left-other"

    putStrLn "ok"
