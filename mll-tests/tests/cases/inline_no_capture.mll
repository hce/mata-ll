-- Call-site inlining substitutes arguments into the candidate's body at
-- the TIR level without alpha-renaming, so a substitution whose argument
-- variables collide with a binder inside the body must be DECLINED.
-- Regression: inlining `add x = \y -> x + y` at the call `add y`
-- substituted x -> y under the body's own `\y`, producing `\y -> y + y`:
-- `map (add y) [1,2,3]` with y = 3 returned [2,4,6] instead of [4,5,6].

add :: Int -> Int -> Int
add x = \y -> x + y

-- the argument variable is literally named like the body's lambda binder
applyAll :: Int -> [Int]
applyAll y = map (add y) [1, 2, 3]

-- control: a non-colliding argument name keeps inlining (same semantics)
applyAll2 :: Int -> [Int]
applyAll2 z = map (add z) [1, 2, 3]

-- collision via an expression argument (not a bare variable)
applyExpr :: Int -> [Int]
applyExpr y = map (add (y * 10)) [1, 2, 3]

main :: IO ()
main = do
    print (applyAll 3)      -- [4,5,6]  (capture gave [2,4,6])
    print (applyAll2 3)     -- [4,5,6]
    print (applyExpr 1)     -- [11,12,13]
    print (add 40 2)        -- 42 (saturated call unaffected)
