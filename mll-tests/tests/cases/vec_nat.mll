-- Test: length-indexed vectors over Peano Nat (DataKinds + GADTs)
-- Locks down the fixed-length behavior: annotated concrete lengths,
-- total vhead/vtail on provably non-empty vectors, a runtime vlen,
-- and a length-preserving (type-changing) vmap.
-- Deliberately NO length arithmetic (no Plus/type families) here.

data Nat = Z | S Nat

data Vec n a where
    VNil  :: Vec 'Z a
    VCons :: a -> Vec n a -> Vec ('S n) a

vhead :: Vec ('S n) a -> a
vhead (VCons x _) = x

-- Total: only accepts vectors with at least one element,
-- and the result type records one element fewer.
vtail :: Vec ('S n) a -> Vec n a
vtail (VCons _ xs) = xs

vlen :: Vec n a -> Integer
vlen VNil = 0
vlen (VCons _ xs) = 1 + vlen xs

-- Length-preserving structural map; the index n flows through unchanged.
vmap :: (a -> b) -> Vec n a -> Vec n b
vmap _ VNil = VNil
vmap f (VCons x xs) = VCons (f x) (vmap f xs)

-- Vectors with their lengths pinned by top-level annotations.
v0 :: Vec 'Z Integer
v0 = VNil

v1 :: Vec ('S 'Z) Integer
v1 = VCons 7 VNil

v3 :: Vec ('S ('S ('S 'Z))) Integer
v3 = VCons 1 (VCons 2 (VCons 3 VNil))

vs :: Vec ('S ('S 'Z)) String
vs = VCons "hello" (VCons "world" VNil)

main :: IO ()
main = do
    -- vlen reflects the annotated type-level length at runtime
    assert (vlen v0 == 0) "vlen v0 should be 0"
    assert (vlen v1 == 1) "vlen v1 should be 1"
    assert (vlen vs == 2) "vlen vs should be 2"
    assert (vlen v3 == 3) "vlen v3 should be 3"
    -- vhead on provably non-empty vectors
    assert (vhead v1 == 7) "vhead v1 should be 7"
    assert (vhead v3 == 1) "vhead v3 should be 1"
    assert (vhead vs == "hello") "vhead vs should be hello"
    -- vtail shortens the length by exactly one and preserves order
    assert (vlen (vtail v3) == 2) "vtail v3 should have length 2"
    assert (vhead (vtail v3) == 2) "second element of v3 should be 2"
    assert (vhead (vtail (vtail v3)) == 3) "third element of v3 should be 3"
    assert (vhead (vtail vs) == "world") "second element of vs should be world"
    -- vmap preserves length and applies f elementwise
    assert (vlen (vmap (\x -> x * 10) v3) == 3) "vmap must preserve length"
    assert (vhead (vmap (\x -> x * 10) v3) == 10) "vmap should scale head to 10"
    assert (vhead (vtail (vmap (\x -> x + 1) v3)) == 3) "vmap (+1) second element should be 3"
    -- vmap may change the element type (a -> b) while keeping n
    assert (vhead (vmap show v3) == "1") "vmap show head should be \"1\""
    assert (vlen (vmap show v3) == 3) "vmap show must preserve length"
    print (vhead v3)
-- expect: 1
