-- GHC's `Monad (Either e)`: Right binds, Left short-circuits — through
-- >>=, >>, do-notation, mapM/traverse/sequence/forM and Control.Monad's
-- generic combinators. `Either e` had Functor, Applicative, Foldable and
-- Traversable instances but no Monad, so `>>=` on `Either String` was
-- "No instance".

import Control.Monad (forM, join, void, unless)

safeDiv :: Int -> Int -> Either String Int
safeDiv _ 0 = Left "divide by zero"
safeDiv a b = Right (a `div` b)

parseDigit :: String -> Either String Int
parseDigit "0" = Right 0
parseDigit "1" = Right 1
parseDigit "2" = Right 2
parseDigit s = Left ("not a digit: " <> s)

calc :: Int -> Int -> Int -> Either String Int
calc a b c = do
    x <- safeDiv a b
    y <- safeDiv x c
    unless (y >= 0) (Left "negative")
    pure (y + 1)

-- A Left never runs the rest: the error call after it is not reached.
shortCircuit :: Either String Int
shortCircuit = do
    _ <- Left "stop"
    error "unreachable"

nested :: Either String (Either String Int)
nested = Right (Right 7)

main :: IO ()
main = do
    print (safeDiv 10 2 >>= \x -> safeDiv x 5)
    print (safeDiv 10 0 >>= \x -> safeDiv x 5)
    print (safeDiv 1 0 >> Right 3, Right 1 >> safeDiv 9 3 :: Either String Int)
    print (calc 100 5 2, calc 100 0 2, calc 100 5 0, calc (-100) 5 2)
    print shortCircuit
    print (mapM parseDigit ["1", "2", "0"], mapM parseDigit ["1", "x", "0"])
    print (traverse (safeDiv 12) [1, 2, 3], traverse (safeDiv 12) [1, 0, 3])
    print (sequence [Right 1, Right 2 :: Either String Int], sequence [Right 1, Left "e", Right 2 :: Either String Int])
    print (forM [3, 4] (\k -> safeDiv 12 k))
    print (join nested, join (Right (Left "inner") :: Either String (Either String Int)))
    print (void (Right 5 :: Either String Int), void (Left "v" :: Either String Int))
    print (fmap (+ 1) (Right 1 :: Either String Int), (+) <$> Right 1 <*> (Right 2 :: Either String Int))
    print (return 4 :: Either String Int, pure 4 :: Either String Int)
