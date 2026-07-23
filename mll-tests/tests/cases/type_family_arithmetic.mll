-- Type-family ARITHMETIC over length-indexed vectors: the unifier reduces
-- closed type families symbolically (over type variables), so `Plus` computes
-- vector lengths at the type level and `vappend` type-checks, runs, and keeps
-- the length sound. (Basic fixed-length Nat/Vec construction and vhead are
-- covered elsewhere; this fixture is about the arithmetic that reduction
-- unlocks.)

data Nat = Z | S Nat

type family Plus n m where
    Plus 'Z     m = m
    Plus ('S n) m = 'S (Plus n m)

data Vec n a where
    VNil  :: Vec 'Z a
    VCons :: a -> Vec n a -> Vec ('S n) a

-- The payoff: the length of the result is `Plus n m`, which only type-checks
-- because the unifier reduces `Plus 'Z m` to `m` (clause 1) and
-- `Plus ('S n) m` to `'S (Plus n m)` (clause 2).
vappend :: Vec n a -> Vec m a -> Vec (Plus n m) a
vappend VNil ys = ys
vappend (VCons x xs) ys = VCons x (vappend xs ys)

-- Nested / deeply-stuck family application: `Plus n (Plus m p)` stays
-- irreducible while symbolic, and the recursion still type-checks.
vappend3 :: Vec n a -> Vec m a -> Vec p a -> Vec (Plus n (Plus m p)) a
vappend3 xs ys zs = vappend xs (vappend ys zs)

vlen :: Vec n a -> Int
vlen VNil = 0
vlen (VCons _ xs) = 1 + vlen xs

vtoList :: Vec n a -> [a]
vtoList VNil = []
vtoList (VCons x xs) = x : vtoList xs

-- A length-sensitive consumer: it demands a provably non-empty vector.
vhead :: Vec ('S n) a -> a
vhead (VCons x _) = x

main :: IO ()
main = do
    let v2 = VCons 1 (VCons 2 VNil)
    let v3 = VCons 3 (VCons 4 (VCons 5 VNil))
    let v5 = vappend v2 v3
    -- The runtime length matches the type-level arithmetic: Plus 2 3 = 5.
    assert (vlen v5 == 5) "vappend length = 2 + 3"
    assert (vlen v5 == vlen v2 + vlen v3) "vlen (vappend xs ys) == vlen xs + vlen ys"
    assert (vtoList v5 == [1, 2, 3, 4, 5]) "vappend preserves order"
    -- vhead on a vappend result whose length is provably >= 1 (Plus of a
    -- non-empty and anything reduces to 'S ...).
    assert (vhead v5 == 1) "vhead of a non-empty vappend"

    -- Three-way append: Plus 1 (Plus 1 1) = 3.
    let w = vappend3 (VCons 10 VNil) (VCons 20 VNil) (VCons 30 VNil)
    assert (vlen w == 3) "vappend3 length = 1 + (1 + 1)"
    assert (vtoList w == [10, 20, 30]) "vappend3 order"

    -- Appending an empty vector on the left (Plus 'Z m = m) is the identity.
    let e = vappend (VNil :: Vec 'Z Int) v3
    assert (vtoList e == [3, 4, 5]) "vappend VNil ys == ys"

    putStrLn "type-family arithmetic ok"
-- expect: type-family arithmetic ok
