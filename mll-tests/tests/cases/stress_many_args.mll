-- Stress test: functions with many arguments

sum15 :: Int -> Int -> Int -> Int -> Int -> Int -> Int -> Int -> Int -> Int -> Int -> Int -> Int -> Int -> Int -> Int
sum15 a1 a2 a3 a4 a5 a6 a7 a8 a9 a10 a11 a12 a13 a14 a15 = a1 + a2 + a3 + a4 + a5 + a6 + a7 + a8 + a9 + a10 + a11 + a12 + a13 + a14 + a15

pack10 :: Int -> Int -> Int -> Int -> Int -> Int -> Int -> Int -> Int -> Int -> (Int, Int)
pack10 a1 a2 a3 a4 a5 a6 a7 a8 a9 a10 = (a1 + a2 + a3 + a4 + a5, a6 + a7 + a8 + a9 + a10)

add5 :: Int -> Int -> Int -> Int -> Int -> Int
add5 a b c d e = a + b + c + d + e

compose5 :: Int -> Int
compose5 n = add5 (add5 n n n n n) (add5 n n n n n) (add5 n n n n n) (add5 n n n n n) (add5 n n n n n)

main :: IO ()
main = do
    assert (sum15 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 == 120) "sum15"
    let r = pack10 1 2 3 4 5 6 7 8 9 10
    assert (fst r == 15) "pack10 fst"
    assert (snd r == 40) "pack10 snd"
    assert (add5 10 20 30 40 50 == 150) "add5"
    -- Partial application
    let f = add5 1 2 3
    assert (f 4 5 == 15) "partial add5"
    assert (compose5 2 == 50) "compose5"
    assert (compose5 0 == 0) "compose5 zero"
    putStrLn "ok"
