-- GHC cgrun018: Either operations
-- Tests Either pattern matching and mapping

safeDiv :: Integer -> Integer -> Either String Integer
safeDiv _ 0 = Left "division by zero"
safeDiv a b = Right (a `div` b)

mapRight :: (Integer -> Integer) -> Either String Integer -> Either String Integer
mapRight _ (Left e)  = Left e
mapRight f (Right x) = Right (f x)

fromRight :: Integer -> Either String Integer -> Integer
fromRight def (Left _)  = def
fromRight _   (Right x) = x

isLeft :: Either String Integer -> Bool
isLeft (Left _)  = True
isLeft (Right _) = False

eqEither :: Either String Integer -> Either String Integer -> Bool
eqEither (Left a)  (Left b)  = a == b
eqEither (Right a) (Right b) = a == b
eqEither _         _         = False

main :: IO ()
main = do
    assert (eqEither (safeDiv 10 2) (Right 5)) "div ok"
    assert (eqEither (safeDiv 10 0) (Left "division by zero")) "div zero"
    assert (eqEither (mapRight (* 2) (Right 5)) (Right 10)) "mapRight Right"
    assert (eqEither (mapRight (* 2) (Left "err")) (Left "err")) "mapRight Left"
    assert (fromRight 0 (Right 42) == 42) "fromRight Right"
    assert (fromRight 0 (Left "err") == 0) "fromRight Left"
    assert (isLeft (Left "err") == True) "isLeft Left"
    assert (isLeft (Right 1) == False) "isLeft Right"
    putStrLn "ok"
