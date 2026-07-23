-- GHC cgrun066: Peano arithmetic with proofs (as boolean checks)

data Nat = Z | S Nat
    deriving (Show, Eq)

toNat :: Int -> Nat
toNat 0 = Z
toNat n = S (toNat (n - 1))

fromNat :: Nat -> Int
fromNat Z     = 0
fromNat (S n) = 1 + fromNat n

addNat :: Nat -> Nat -> Nat
addNat Z     m = m
addNat (S n) m = S (addNat n m)

mulNat :: Nat -> Nat -> Nat
mulNat Z     _ = Z
mulNat (S n) m = addNat m (mulNat n m)

leNat :: Nat -> Nat -> Bool
leNat Z     _     = True
leNat _     Z     = False
leNat (S n) (S m) = leNat n m

eqNat :: Nat -> Nat -> Bool
eqNat Z Z         = True
eqNat (S n) (S m) = eqNat n m
eqNat _ _         = False

subNat :: Nat -> Nat -> Nat
subNat Z     _     = Z
subNat n     Z     = n
subNat (S n) (S m) = subNat n m

main :: IO ()
main = do
    assert (fromNat (toNat 0) == 0) "toNat/fromNat 0"
    assert (fromNat (toNat 5) == 5) "toNat/fromNat 5"
    assert (fromNat (addNat (toNat 3) (toNat 4)) == 7) "3 + 4 = 7"
    assert (fromNat (mulNat (toNat 3) (toNat 4)) == 12) "3 * 4 = 12"
    assert (fromNat (mulNat (toNat 0) (toNat 99)) == 0) "0 * 99 = 0"
    -- commutativity of add (spot checks)
    assert (eqNat (addNat (toNat 3) (toNat 5)) (addNat (toNat 5) (toNat 3))) "add commutes"
    -- distributivity spot check: 2*(3+4) = 2*3+2*4
    let two = toNat 2
    let three = toNat 3
    let four = toNat 4
    assert (eqNat (mulNat two (addNat three four)) (addNat (mulNat two three) (mulNat two four))) "distributive"
    -- leNat
    assert (leNat (toNat 3) (toNat 5)) "3 <= 5"
    assert (not (leNat (toNat 6) (toNat 2))) "not 6 <= 2"
    assert (leNat (toNat 4) (toNat 4)) "4 <= 4"
    -- subNat
    assert (fromNat (subNat (toNat 7) (toNat 3)) == 4) "7 - 3 = 4"
    assert (fromNat (subNat (toNat 2) (toNat 9)) == 0) "monus: 2 - 9 = 0"
    putStrLn "ok"
