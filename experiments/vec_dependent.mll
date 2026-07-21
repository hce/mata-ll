-- A truly dependent type: a length-indexed vector `Vec n a`.
--
-- This builds directly on `Nat`. Because `Nat` is a parameterless data type,
-- mata-ll PROMOTES it (DataKinds): the type `Nat` also becomes a kind, with
-- `'Zero :: Nat` and `'Succ :: Nat -> Nat` as type-level values. `Vec` is then
-- indexed by one of those type-level Nats, so the LENGTH lives in the type and
-- the compiler tracks it — a type that depends on a value is what makes this
-- "dependent". (`Nat` itself, from Nat.mll, is just the ordinary inductive
-- type; promotion is what lifts it to the type level.)
import Nat (Nat(..))

-- The vector: a GADT whose constructors pin the length index.
--   VNil  has length 'Zero
--   VCons grows a length-n vector to length 'Succ n
data Vec n a where
    VNil  :: Vec 'Zero a
    VCons :: a -> Vec n a -> Vec ('Succ n) a

-- Type-level addition over the PROMOTED Nat. A closed type family reduces
-- symbolically during unification, so `Plus` computes a length at compile time.
-- (This is the type-level twin of Nat.addNat.)
type family Plus n m where
    Plus 'Zero     m = m
    Plus ('Succ n) m = 'Succ (Plus n m)

-- Safe head: its argument type `Vec ('Succ n) a` demands a provably NON-EMPTY
-- vector, so there is no VNil clause to write — and none is missing, because
-- the compiler knows VNil cannot have length 'Succ n. `vhead VNil` is a
-- compile-time type error, not a runtime crash. (See the block at the bottom.)
vhead :: Vec ('Succ n) a -> a
vhead (VCons x _) = x

-- Length-preserving map: same index `n` in and out, enforced by the signature.
vmap :: (a -> b) -> Vec n a -> Vec n b
vmap _ VNil         = VNil
vmap f (VCons x xs) = VCons (f x) (vmap f xs)

-- Zip: both arguments (and the result) share the length `n`, so zipping
-- vectors of different lengths is rejected at the CALL site. The mixed
-- VNil/VCons clauses are unreachable under the shared index and need not be
-- written — the match is still exhaustive.
vzip :: Vec n a -> Vec n b -> Vec n (a, b)
vzip VNil         VNil         = VNil
vzip (VCons x xs) (VCons y ys) = VCons (x, y) (vzip xs ys)

-- Append: the result length is `Plus m n`, and it type-checks ONLY because the
-- family reduces `Plus 'Zero n` to `n` and `Plus ('Succ m) n` to `'Succ (...)`
-- in step with the two clauses. The length is proven correct, not asserted.
vappend :: Vec m a -> Vec n a -> Vec (Plus m n) a
vappend VNil         ys = ys
vappend (VCons x xs) ys = VCons x (vappend xs ys)

-- Forget the length to recover an ordinary list, for printing.
vtoList :: Vec n a -> [a]
vtoList VNil         = []
vtoList (VCons x xs) = x : vtoList xs

main :: IO ()
main = do
    let v3 = VCons 10 (VCons 20 (VCons 30 VNil))  -- Vec ('Succ ('Succ ('Succ 'Zero))) Integer
    let w2 = VCons 40 (VCons 50 VNil)             -- Vec ('Succ ('Succ 'Zero)) Integer
    -- vhead is legal here: v3's length is 'Succ (…), provably non-empty.
    putStrLn ("vhead v3:        " <> show (vhead v3))
    putStrLn ("vmap (*2) v3:    " <> show (vtoList (vmap (\x -> x * 2) v3)))
    -- vzip demands equal lengths; pair v3 with another length-3 vector.
    let u3 = VCons 1 (VCons 2 (VCons 3 VNil))
    putStrLn ("vzip v3 u3:      " <> show (vtoList (vzip v3 u3)))
    -- vappend adds the lengths at the type level: 3 + 2 = 5.
    putStrLn ("vappend v3 w2:   " <> show (vtoList (vappend v3 w2)))

-- The dependent guarantees, as things the compiler REJECTS. Uncomment either
-- line to see the error the length index produces:
--
--   vhead VNil
--     -> Type error: Cannot unify ''Succ a' with ''Zero'
--        (VNil has length 'Zero; vhead needs 'Succ n)
--
--   vzip v3 w2          -- lengths 3 and 2
--     -> Type error: Cannot unify ''Succ 'Zero' with ''Zero'
--        (the shared index n cannot be both 3 and 2)
