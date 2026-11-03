-- lua-compat-skip: luajit
-- An Int range that reaches maxBound (or minBound downward) terminates,
-- and `[x ..]` / `[x, y ..]` at Int are bounded (N11): the Prelude's
-- helpers incremented past maxBound, wrapped, and `n > m` never held, so
-- `length [maxBound - 2 .. maxBound]` hung and `take 5` of it printed
-- wrapped elements; `[maxBound - 1 ..]` had no end. GHC.Enum's Int
-- instance is transcribed now (`eftInt` stops on `x == y` before the
-- increment, `efdtIntUp`/`efdtIntDn` on `x > y - delta`, `enumFrom` runs
-- to maxBound, `enumFromThen` to the bound in its direction) — every
-- printed value is GHC's, including the wrapping-delta stepped shapes
-- (`[minBound, maxBound .. maxBound]`), the `[x1]`/`[]` short answers
-- when the limit falls below the second element, the fused pipelines
-- (sum/length/foldl' over a range ending at maxBound), and the unfused
-- consumers (take, last, zip, takeWhile, foldr). The Integer range is
-- the control. LuaJIT skip: the Int bounds are doubles there (see
-- enum_int_bounds_terminate for the both-host termination assertions).
module Main where

main :: IO ()
main = do
    print (take 5 [maxBound - 2 .. maxBound :: Int])
    print (length [maxBound - 2 .. maxBound :: Int])
    print (sum [maxBound - 2 .. maxBound :: Int])
    print (take 5 [maxBound - 1 ..] :: [Int])
    print (take 5 [maxBound - 3, maxBound - 1 ..] :: [Int])
    print [maxBound - 1, maxBound .. maxBound :: Int]
    print [maxBound - 5, maxBound - 2 .. maxBound :: Int]
    print [minBound + 2, minBound + 1 .. minBound :: Int]
    print (take 5 [minBound + 1, minBound ..] :: [Int])
    print (length [minBound + 2, minBound + 1 .. minBound :: Int])
    print (take 3 [9223372036854775805 .. 9223372036854775807 :: Integer])
    print (length (filter even [maxBound - 4 .. maxBound :: Int]))
    print (take 3 [maxBound ..] :: [Int])
    print [minBound, minBound + 1 .. minBound + 3 :: Int]
    print [maxBound, maxBound - 1 .. maxBound - 3 :: Int]
    print (sum (map (\x -> x - maxBound) [maxBound - 3 .. maxBound :: Int]))
    print [maxBound .. maxBound - 1 :: Int]
    print [maxBound .. maxBound :: Int]
    print [1, maxBound .. maxBound :: Int]
    print [maxBound - 2, maxBound .. maxBound - 1 :: Int]
    print (take 3 [minBound ..] :: [Int])
    print [minBound .. minBound :: Int]
    print [minBound + 1 .. minBound :: Int]
    print [minBound + 1, minBound .. minBound :: Int]
    print [-1, minBound .. minBound :: Int]
    print [minBound + 2, minBound .. minBound + 1 :: Int]
    print (take 5 [minBound, minBound + 2 ..] :: [Int])
    print (take 5 [maxBound, maxBound - 2 ..] :: [Int])
    print (length [maxBound - 5, maxBound - 2 .. maxBound :: Int])
    print (sum [minBound + 3, minBound + 4 .. minBound + 5 :: Int])
    print (take 3 [maxBound - 5 .. maxBound :: Int], length [minBound .. minBound + 2 :: Int])
    print (foldl' (+) 0 (map (`div` 2) [maxBound - 3 .. maxBound :: Int]))
    print (length (takeWhile (< maxBound) [maxBound - 3 ..] :: [Int]))
    print (foldr (\x acc -> x - maxBound : acc) [] [maxBound - 2 .. maxBound :: Int])
    print (length (takeWhile (> minBound) [minBound + 3, minBound + 2 ..] :: [Int]))
    print (zip [maxBound - 1 ..] [10, 20, 30 :: Int] :: [(Int, Int)])
    print [maxBound - 1, maxBound .. maxBound - 1 :: Int]
    print [maxBound, maxBound - 1 .. maxBound :: Int]
    print [minBound, maxBound .. maxBound :: Int]
    print (take 3 [minBound, maxBound ..] :: [Int])
    print [minBound, 0 .. maxBound :: Int]
    print [maxBound, minBound .. minBound :: Int]
    print (take 3 [maxBound, minBound ..] :: [Int])
    print [maxBound, 0 .. minBound :: Int]
    print (take 3 [0, maxBound ..] :: [Int])
    print [0, maxBound .. maxBound :: Int]
    print [minBound, -1 .. maxBound :: Int]
    print [maxBound, 1 .. minBound :: Int]
    print (take 4 [minBound + 1, minBound + 3 .. maxBound :: Int])
    print (length [maxBound - 7, maxBound - 4 .. maxBound :: Int], last [maxBound - 7, maxBound - 4 .. maxBound :: Int])
    print (length [minBound + 7, minBound + 4 .. minBound :: Int], last [minBound + 7, minBound + 4 .. minBound :: Int])
    print (length [maxBound - 8, maxBound - 4 .. maxBound :: Int], last [maxBound - 8, maxBound - 4 .. maxBound :: Int])

-- expect: [9223372036854775805,9223372036854775806,9223372036854775807]
-- expect: 3
-- expect: 9223372036854775802
-- expect: [9223372036854775806,9223372036854775807]
-- expect: [9223372036854775804,9223372036854775806]
-- expect: [9223372036854775806,9223372036854775807]
-- expect: [9223372036854775802,9223372036854775805]
-- expect: [-9223372036854775806,-9223372036854775807,-9223372036854775808]
-- expect: [-9223372036854775807,-9223372036854775808]
-- expect: 3
-- expect: [9223372036854775805,9223372036854775806,9223372036854775807]
-- expect: 2
-- expect: [9223372036854775807]
-- expect: [-9223372036854775808,-9223372036854775807,-9223372036854775806,-9223372036854775805]
-- expect: [9223372036854775807,9223372036854775806,9223372036854775805,9223372036854775804]
-- expect: -6
-- expect: []
-- expect: [9223372036854775807]
-- expect: [1,9223372036854775807]
-- expect: [9223372036854775805]
-- expect: [-9223372036854775808,-9223372036854775807,-9223372036854775806]
-- expect: [-9223372036854775808]
-- expect: []
-- expect: [-9223372036854775807,-9223372036854775808]
-- expect: [-1,-9223372036854775808]
-- expect: [-9223372036854775806]
-- expect: [-9223372036854775808,-9223372036854775806,-9223372036854775804,-9223372036854775802,-9223372036854775800]
-- expect: [9223372036854775807,9223372036854775805,9223372036854775803,9223372036854775801,9223372036854775799]
-- expect: 2
-- expect: -9223372036854775796
-- expect: ([9223372036854775802,9223372036854775803,9223372036854775804],3)
-- expect: -6
-- expect: 3
-- expect: [-2,-1,0]
-- expect: 3
-- expect: [(9223372036854775806,10),(9223372036854775807,20)]
-- expect: [9223372036854775806]
-- expect: [9223372036854775807]
-- expect: [-9223372036854775808,9223372036854775807]
-- expect: [-9223372036854775808,9223372036854775807]
-- expect: [-9223372036854775808,0]
-- expect: [9223372036854775807,-9223372036854775808]
-- expect: [9223372036854775807,-9223372036854775808]
-- expect: [9223372036854775807,0,-9223372036854775807]
-- expect: [0,9223372036854775807]
-- expect: [0,9223372036854775807]
-- expect: [-9223372036854775808,-1,9223372036854775806]
-- expect: [9223372036854775807,1,-9223372036854775805]
-- expect: [-9223372036854775807,-9223372036854775805,-9223372036854775803,-9223372036854775801]
-- expect: (3,9223372036854775806)
-- expect: (3,-9223372036854775807)
-- expect: (3,9223372036854775807)
