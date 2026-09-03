-- A type ascription's variables are rigid while the expression is checked
-- against them (`(5 :: a)` is rejected) and then instantiated afresh for
-- the use, as GHC quantifies them: `(id :: a -> a) 5` is legal. It used
-- to be rejected as "cannot match rigid 'a' with Int", blaming the
-- enclosing function's signature.

twice :: (a -> a) -> a -> a
twice f x = f (f x)

main :: IO ()
main = do
    print ((id :: a -> a) (5 :: Int))
    print (((\x -> [x]) :: b -> [b]) "s")
    print ((twice :: (c -> c) -> c -> c) (+ 1) (10 :: Int))
    let f = (\g -> g 3) :: (Int -> Int) -> Int
    print (f (+ 1))
    print (map (id :: d -> d) [True, False])
    print ((const :: p -> q -> p) "keep" (1 :: Int), (const :: p -> q -> p) (2 :: Int) "drop")
