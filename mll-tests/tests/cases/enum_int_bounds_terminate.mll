-- Termination of the Int ranges at the bounds on BOTH hosts (N11). On
-- LuaJIT the Int bounds are doubles — `maxBound - 2` is 2^63 there — so
-- the elements are not GHC's; what holds on every host is that every
-- walk stops: the `[x .. maxBound]` and `[x ..]` forms exit on the
-- equality (never computing maxBound + 1), the stepped forms exit on
-- `x > y - delta`, and a fused pipeline over a range ending at maxBound
-- exits the same way. The stepped shapes step by 2048 so both leading
-- elements are distinct doubles on LuaJIT (a step that collapses to zero
-- there is an infinite list, as GHC's rule says of a zero step). Every
-- line prints True.
module Main where

main :: IO ()
main = do
    print (length [maxBound .. maxBound :: Int] == 1)
    print (last [maxBound - 2 .. maxBound :: Int] == maxBound)
    print (take 2 [maxBound ..] == [maxBound :: Int])
    print (last (take 3 [maxBound - 1 ..]) == (maxBound :: Int))
    print (head [minBound, minBound + 1 ..] == (minBound :: Int))
    print (last [maxBound - 4096, maxBound - 2048 .. maxBound :: Int] == maxBound, length [maxBound - 4096, maxBound - 2048 .. maxBound :: Int] == 3)
    print (last [minBound + 4096, minBound + 2048 .. minBound :: Int] == minBound, length [minBound + 4096, minBound + 2048 .. minBound :: Int] == 3)
    print (last [minBound .. minBound + 2 :: Int] == minBound + 2)
    print (length (filter even [maxBound - 4 .. maxBound :: Int]) <= 3)
    print (foldl' max 0 [maxBound - 3 .. maxBound :: Int] == maxBound)
    print (sum (map (\x -> maxBound - x) [maxBound - 3 .. maxBound :: Int]) <= 6)
    print (length [maxBound - 3 .. maxBound :: Int] <= 4)
    print (take 3 [maxBound, maxBound - 2048 ..] == [maxBound, maxBound - 2048, maxBound - 4096 :: Int])
    print (take 3 [minBound, minBound + 2048 ..] == [minBound, minBound + 2048, minBound + 4096 :: Int])
    print (null [maxBound - 2048 .. maxBound - 4096 :: Int], null [minBound + 2048, minBound + 4096 .. minBound :: Int])
    print (sum (filter (\x -> x >= maxBound - 4096) [maxBound - 4096, maxBound - 2048 .. maxBound :: Int]) == 3 * maxBound - 6144)

-- expect: True
-- expect: True
-- expect: True
-- expect: True
-- expect: True
-- expect: (True,True)
-- expect: (True,True)
-- expect: True
-- expect: True
-- expect: True
-- expect: True
-- expect: True
-- expect: True
-- expect: True
-- expect: (True,True)
-- expect: True
