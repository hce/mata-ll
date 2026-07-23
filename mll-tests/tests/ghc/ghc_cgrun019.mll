-- GHC cgrun019: List comprehensions (advanced)
-- Tests multiple generators, guards, and nested comprehensions

pythag :: Int -> [(Int, Int, Int)]
pythag n = [(a, b, c) | c <- [1..n], b <- [1..c], a <- [1..b], a*a + b*b == c*c]

main :: IO ()
main = do
    let triples = pythag 20
    assert (length triples == 6) "6 triples up to 20"
    assert (elem (3, 4, 5) triples) "3,4,5"
    assert (elem (5, 12, 13) triples) "5,12,13"
    assert (elem (6, 8, 10) triples) "6,8,10"

    -- List of squares of even numbers
    let evensq = [x * x | x <- [1..10], x `mod` 2 == 0]
    assert (evensq == [4, 16, 36, 64, 100]) "even squares"

    -- Cartesian product
    let pairs = [(x, y) | x <- [1, 2, 3], y <- [10, 20]]
    assert (length pairs == 6) "cartesian length"

    putStrLn "ok"
