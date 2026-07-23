-- DataKinds step 2: a promoted data type gets a REAL kind. `data Nat` gives
-- the kind `Nat` (with `'Z :: Nat`, `'S :: Nat -> Nat`), so a length index is
-- checked to be specifically a `Nat`. This fixture pins the POSITIVE side:
-- a well-kinded Nat index works, AND the very same `Nat` is still usable as an
-- ordinary VALUE type at runtime (the type/kind duality). Ill-kinded indices
-- (`Vec 'True`, `Plus 'True …`) are rejected — see the run_mll.rs error tests.

data Nat = Z | S Nat

-- `Nat` as a VALUE type: build and consume runtime Nats.
toInt :: Nat -> Int
toInt Z     = 0
toInt (S n) = 1 + toInt n

fromInt :: Int -> Nat
fromInt 0 = Z
fromInt n = S (fromInt (n - 1))

-- `Nat` as a KIND: the index `n` of `Vec` is inferred to have kind `Nat`
-- (from the constructor return types applying it to `'Z` / `'S n`).
data Vec n a where
    VNil  :: Vec 'Z a
    VCons :: a -> Vec n a -> Vec ('S n) a

vlen :: Vec n a -> Int
vlen VNil        = 0
vlen (VCons _ xs) = 1 + vlen xs

vhead :: Vec ('S n) a -> a
vhead (VCons x _) = x

-- A promoted index written out explicitly is accepted because its kind is Nat.
v3 :: Vec ('S ('S ('S 'Z))) Int
v3 = VCons 1 (VCons 2 (VCons 3 VNil))

main :: IO ()
main = do
    -- Nat as a value
    assert (toInt (S (S Z)) == 2) "Nat value: S (S Z) = 2"
    assert (toInt (fromInt 5) == 5) "Nat value round-trip"
    -- Nat as a kind index
    assert (vlen v3 == 3) "Nat-kinded index: vlen v3 = 3"
    assert (vhead v3 == 1) "vhead of a Nat-indexed vector"
    putStrLn "promoted Nat kind ok"
-- expect: promoted Nat kind ok
