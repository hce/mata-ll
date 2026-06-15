-- GHC tc004: Superclass usage
-- Tests accessing superclass methods through subclass constraints

class Eq a => MyOrd a where
    cmp :: a -> a -> Ordering

data Temp = Cold | Warm | Hot
    deriving (Show, Eq)

instance MyOrd Temp where
    cmp Cold Cold = EQ
    cmp Cold _    = LT
    cmp Hot Hot   = EQ
    cmp Hot _     = GT
    cmp Warm Cold = GT
    cmp Warm Hot  = LT
    cmp Warm Warm = EQ

myMin :: MyOrd a => a -> a -> a
myMin a b = case cmp a b of
    LT -> a
    _  -> b

myMax :: MyOrd a => a -> a -> a
myMax a b = case cmp a b of
    GT -> a
    _  -> b

-- Uses Eq (superclass) through MyOrd constraint
myElem :: MyOrd a => a -> [a] -> Bool
myElem _ [] = False
myElem x (y:ys)
    | x == y    = True
    | otherwise = myElem x ys

main :: IO ()
main = do
    assert (cmp Cold Hot == LT) "cmp cold hot"
    assert (cmp Hot Cold == GT) "cmp hot cold"
    assert (cmp Warm Warm == EQ) "cmp warm warm"
    assert (myMin Cold Hot == Cold) "myMin"
    assert (myMax Cold Hot == Hot) "myMax"
    assert (myElem Warm [Cold, Warm, Hot] == True) "myElem found"
    assert (myElem Hot [Cold, Warm] == False) "myElem not found"
    putStrLn "ok"
