-- Self-tail-call → loop conversion (opt pass 5): swap-style tail calls,
-- where a cascading parameter update would read an already-overwritten
-- value. The conversion must use one simultaneous multiple assignment.

-- Classic swap accumulator: fib via (a, b) -> (b, a+b).
fibGo :: Int -> Int -> Int -> Int
fibGo 0 a b = a
fibGo n a b = fibGo (n - 1) b (a + b)

-- Pure swap: after an odd number of steps the arguments are exchanged.
swapN :: Int -> Int -> Int -> (Int, Int)
swapN 0 a b = (a, b)
swapN n a b = swapN (n - 1) b a

main :: IO ()
main = do
    assert (fibGo 10 0 1 == 55) "fib 10 via swap-style accumulator"
    assert (fibGo 30 0 1 == 832040) "fib 30 via swap-style accumulator"
    assert (swapN 7 1 2 == (2, 1)) "odd swap count exchanges"
    assert (swapN 8 1 2 == (1, 2)) "even swap count restores"
