-- Tuple construction and pattern matching
swap :: (a, b) -> (b, a)
swap (x, y) = (y, x)

addPair :: (Int, Int) -> Int
addPair (a, b) = a + b

-- Nested tuples
nested :: ((Int, String), Bool) -> Int
nested ((n, _), _) = n

-- Tuples in let bindings
letTuple :: Int -> Int
letTuple _ = let p = (10, 20) in fst p + snd p

-- 3-tuples
triple :: (Int, Int, Int) -> Int
triple (a, b, c) = a + b + c

main :: IO ()
main = do
    -- Basic construction and fst/snd
    assert (fst (1, 2) == 1) "fst"
    assert (snd (1, 2) == 2) "snd"

    -- Pattern matching
    assert (addPair (3, 4) == 7) "addPair"
    assert (fst (swap (1, 2)) == 2) "swap fst"
    assert (snd (swap (1, 2)) == 1) "swap snd"

    -- Nested tuples
    assert (nested ((42, "hi"), True) == 42) "nested"

    -- Let binding
    assert (letTuple 0 == 30) "letTuple"

    -- 3-tuples
    assert (triple (1, 2, 3) == 6) "triple"

    -- Tuples in lists
    let pairs = (1, "a") : (2, "b") : (3, "c") : []
    assert (fst (head pairs) == 1) "tuple in list"
    assert (snd (head pairs) == "a") "tuple in list snd"
    assert (length pairs == 3) "tuple list length"
