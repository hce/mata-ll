-- Test: Type-level naturals via DataKinds
-- Promoted 'Z and 'S used as type-level Peano numbers

data Nat = Z | S Nat

data Vec n a where
    VNil  :: Vec 'Z a
    VCons :: a -> Vec n a -> Vec ('S n) a

vhead :: Vec ('S n) a -> a
vhead (VCons x _) = x

vtail :: Vec ('S n) a -> Vec n a
vtail (VCons _ xs) = xs

-- A vector we know has at least two elements
vSecond :: Vec ('S ('S n)) a -> a
vSecond (VCons _ (VCons y _)) = y

vlength :: Vec n a -> Int
vlength VNil = 0
vlength (VCons _ xs) = 1 + vlength xs

main :: IO ()
main = do
    let v3 = VCons 10 (VCons 20 (VCons 30 VNil))
    putStrLn (show (vhead v3))
    putStrLn (show (vSecond v3))
    putStrLn (show (vlength v3))
    let v1 = VCons "hello" VNil
    putStrLn (vhead v1)
    putStrLn (show (vlength v1))
    putStrLn (show (vlength VNil))
-- expect: 10
-- expect: 20
-- expect: 3
-- expect: hello
-- expect: 1
-- expect: 0
