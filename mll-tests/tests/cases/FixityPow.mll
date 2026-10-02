module FixityPow (powTotal, powMixed) where
import Prelude hiding ((^), (<$))

(^) :: Int -> Int -> Int
a ^ b = a - b

infixr 5 <$
(<$) :: Int -> Int -> Int
a <$ b = a - b

powTotal :: Int
powTotal = 10 ^ 3 ^ 2

powMixed :: Int
powMixed = 2 * 10 ^ 3 + (10 <$ 3 <$ 2)
