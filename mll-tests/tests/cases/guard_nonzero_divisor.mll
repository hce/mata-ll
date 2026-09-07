-- A conditional whose guard establishes its divisor nonzero (`le > ls`
-- for `mod (le - ls)`) is evaluated eagerly in a lazy position — the
-- division cannot trap there. The boundary: `>=` establishes nothing, so
-- that shape stays a suspension, and a never-demanded `x mod (a - b)`
-- with a == b must never run (GHC never evaluates it).

wrap :: Int -> Int -> Int -> Int -> Int
wrap hl nPos leFP lsFP =
    let fPos = if hl == 1 && nPos >= leFP && leFP > lsFP then lsFP + ((nPos - lsFP) `mod` (leFP - lsFP)) else nPos
    in fPos

-- The `>=` guard does not make `a - b` nonzero: with a == b the modulus
-- must stay unevaluated when nothing demands it.
unsafeIfEager :: Int -> Int -> Int -> Int
unsafeIfEager a b x =
    let r = if a >= b then x `mod` (a - b) else 0
    in if a > b then r else 0

-- `/= 0` and `0 <` facts.
byNonzero :: Int -> Int -> Int
byNonzero d x = let q = if d /= 0 then x `div` d else 0 in q

byPositive :: Int -> Int -> Int
byPositive d x = let q = if 0 < d then x `rem` d else x in q

main :: IO ()
main = do
    print (wrap 1 10 8 2, wrap 1 5 8 2, wrap 0 10 8 2, wrap 1 10 8 8)
    print (unsafeIfEager 3 3 7, unsafeIfEager 5 3 7)
    print (byNonzero 0 9, byNonzero 4 9, byPositive 0 9, byPositive 4 9)
