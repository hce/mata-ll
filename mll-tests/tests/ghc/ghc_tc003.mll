-- GHC tc003: Typeclass with multiple instances
-- Tests user-defined typeclass dispatch

class Container f where
    empty_ :: f a
    insert_ :: a -> f a -> f a
    toList_ :: f a -> [a]

data Stack a = Stack [a]
    deriving (Show, Eq)

instance Container Stack where
    empty_ = Stack []
    insert_ x (Stack xs) = Stack (x : xs)
    toList_ (Stack xs) = xs

-- Test stack separately to prove typeclass dispatch works
stackTest :: [Int]
stackTest = toList_ (insert_ 3 (insert_ 2 (insert_ 1 (empty_ :: Stack Int))))

main :: IO ()
main = do
    -- Stack via typeclass
    let s0 = empty_ :: Stack Int
    let s1 = insert_ 1 s0
    let s2 = insert_ 2 s1
    let s3 = insert_ 3 s2
    assert (toList_ s3 == [3, 2, 1]) "stack lifo"
    assert (s0 == Stack ([] :: [Int])) "stack empty eq"
    assert (stackTest == [3, 2, 1]) "stack via function"

    -- Multiple insert/toList cycles
    let s4 = insert_ 10 (insert_ 20 (empty_ :: Stack Int))
    assert (toList_ s4 == [10, 20]) "stack 2 elems"
    assert (length (toList_ s4) == 2) "stack length"

    putStrLn "ok"
