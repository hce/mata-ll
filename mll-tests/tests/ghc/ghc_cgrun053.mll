-- GHC cgrun053: Church encoding of booleans and pairs via lambdas

churchTrue :: a -> a -> a
churchTrue t _ = t

churchFalse :: a -> a -> a
churchFalse _ f = f

churchAnd :: (Bool -> Bool -> Bool) -> (Bool -> Bool -> Bool) -> Bool -> Bool -> Bool
churchAnd p q t f = p (q t f) f

churchOr :: (Bool -> Bool -> Bool) -> (Bool -> Bool -> Bool) -> Bool -> Bool -> Bool
churchOr p q t f = p t (q t f)

churchNot :: (Bool -> Bool -> Bool) -> Bool -> Bool -> Bool
churchNot p t f = p f t

decodeBool :: (Bool -> Bool -> Bool) -> Bool
decodeBool cb = cb True False

-- Church pairs
churchPair :: a -> b -> (a -> b -> c) -> c
churchPair x y f = f x y

churchFstF :: Integer -> Integer -> Integer
churchFstF x y = x

churchSndF :: Integer -> Integer -> Integer
churchSndF x y = y

churchFst :: ((Integer -> Integer -> Integer) -> Integer) -> Integer
churchFst p = p churchFstF

churchSnd :: ((Integer -> Integer -> Integer) -> Integer) -> Integer
churchSnd p = p churchSndF

main :: IO ()
main = do
    assert (decodeBool churchTrue == True) "churchTrue"
    assert (decodeBool churchFalse == False) "churchFalse"
    assert (decodeBool (churchAnd churchTrue churchTrue) == True) "and T T"
    assert (decodeBool (churchAnd churchTrue churchFalse) == False) "and T F"
    assert (decodeBool (churchAnd churchFalse churchTrue) == False) "and F T"
    assert (decodeBool (churchOr churchFalse churchTrue) == True) "or F T"
    assert (decodeBool (churchOr churchFalse churchFalse) == False) "or F F"
    assert (decodeBool (churchNot churchTrue) == False) "not T"
    assert (decodeBool (churchNot churchFalse) == True) "not F"
    let p = churchPair (42 :: Integer) (99 :: Integer)
    assert (churchFst p == 42) "fst pair"
    assert (churchSnd p == 99) "snd pair"
    putStrLn "ok"
