-- Peano naturals: an ordinary recursive (inductive) type of kind Type.
--
-- Split into its own module so `nat_hkt.mll` can `import Nat` and use it as the
-- element type of a higher-kinded container. `Nat(..)` in the export list means
-- the constructors `Zero`/`Succ` are exported too, so importers can pattern
-- match and build values, not just pass them around opaquely.
module Nat
    ( Nat(..)
    , addNat
    , toInt
    , fromInt
    ) where

data Nat = Zero | Succ Nat

addNat :: Nat -> Nat -> Nat
addNat Zero     m = m
addNat (Succ n) m = Succ (addNat n m)

toInt :: Nat -> Int
toInt Zero     = 0
toInt (Succ n) = 1 + toInt n

fromInt :: Int -> Nat
fromInt n
  | n <= 0    = Zero
  | otherwise = Succ (fromInt (n - 1))
