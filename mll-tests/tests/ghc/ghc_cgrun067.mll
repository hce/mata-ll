-- GHC cgrun067: Natural number arithmetic via ADT (BigNat in base 10)
-- Represents non-negative integers as lists of digits (least significant first)

type Digit = Integer
type BigNat = [Digit]   -- digits in base 10, LSB first

fromInteger_ :: Integer -> BigNat
fromInteger_ 0 = [0]
fromInteger_ n = go n
  where
    go 0 = []
    go m = (m `mod` 10) : go (m `div` 10)

toInteger_ :: BigNat -> Integer
toInteger_ [] = 0
toInteger_ (d:ds) = d + 10 * toInteger_ ds

addBig :: BigNat -> BigNat -> BigNat
addBig xs ys = addWithCarry xs ys 0

addWithCarry :: BigNat -> BigNat -> Integer -> BigNat
addWithCarry [] [] 0 = []
addWithCarry [] [] c = [c]
addWithCarry (x:xs) [] c = let s = x + c in (s `mod` 10) : addWithCarry xs [] (s `div` 10)
addWithCarry [] (y:ys) c = let s = y + c in (s `mod` 10) : addWithCarry [] ys (s `div` 10)
addWithCarry (x:xs) (y:ys) c =
    let s = x + y + c
    in  (s `mod` 10) : addWithCarry xs ys (s `div` 10)

mulBig :: BigNat -> BigNat -> BigNat
mulBig _ [] = [0]
mulBig [] _ = [0]
mulBig xs (y:ys) =
    let partial = map (* y) xs
        carry   = normalise partial
        shifted = 0 : mulBig xs ys
    in  addBig carry shifted

normalise :: [Integer] -> BigNat
normalise ds = go ds 0
  where
    go [] 0 = []
    go [] c = [c]
    go (d:rest) c =
        let total = d + c
        in  (total `mod` 10) : go rest (total `div` 10)

main :: IO ()
main = do
    let a = fromInteger_ 123
    let b = fromInteger_ 456
    assert (toInteger_ a == 123) "fromInteger 123"
    assert (toInteger_ (addBig a b) == 579) "123 + 456 = 579"
    assert (toInteger_ (addBig (fromInteger_ 999) (fromInteger_ 1)) == 1000) "carry"
    assert (toInteger_ (mulBig (fromInteger_ 12) (fromInteger_ 34)) == 408) "12 * 34"
    assert (toInteger_ (mulBig (fromInteger_ 100) (fromInteger_ 100)) == 10000) "100*100"
    assert (toInteger_ (mulBig (fromInteger_ 0) (fromInteger_ 999)) == 0) "0 * 999"
    assert (toInteger_ (addBig (fromInteger_ 0) (fromInteger_ 0)) == 0) "0 + 0"
    putStrLn "ok"
