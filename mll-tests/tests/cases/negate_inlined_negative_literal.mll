-- A prefix minus applied to a negative literal that arrives through
-- inlining: `subNeg x y = y - (-x)` at `subNeg (-5) n` substitutes the
-- already-folded literal -5 for x, so the emitter met `negate (-5)` with
-- nothing left to fold and printed `n - (--5)` — `--` opens a Lua comment
-- and the module did not load. Pinned at Int and Number and through the
-- Integer method path, with nested negation `-(-(-x))`, negative literal
-- arguments at several arities and at a parameter used twice, `negate` of
-- a clause's negative-literal result and of an opaque negative value, and
-- a binary minus whose right operand is a negated negative literal. Every
-- call site is behind a barrier (`pure`, `opaque`) so that the constant
-- folder cannot reduce the call before the inliner sees it.

opaque :: a -> a
opaque v = if length [] == 0 then v else v

subNeg :: Int -> Int -> Int
subNeg x y = y - (-x)

subNegN :: Number -> Number -> Number
subNegN x m = m - (-x)

subNegI :: Integer -> Integer -> Integer
subNegI x y = y - (-x)

neg1 :: Int -> Int
neg1 x = -x

neg3 :: Int -> Int
neg3 x = -(-(-x))

neg3N :: Number -> Number
neg3N x = -(-(-x))

three :: Int -> Int -> Int -> Int
three a b c = -a + (-b) - (-c)

twice :: Int -> Int
twice x = -x - (-x) * 2

step :: Int -> Int
step 0 = 0
step 1 = -1
step n = n

main :: IO ()
main = do
    n <- pure 3
    m <- pure 1.5
    i <- pure (7 :: Integer)
    print (subNeg (-5) n)
    print (subNegN (-2.5) m)
    print (subNegI (-5) i)
    print (subNegI (-100000000000000000000) i)
    print (neg1 (-7))
    print (neg3 (-7))
    print (neg3N (-0.25))
    print (three (-1) (-2) (-3))
    print (twice (-4))
    print (negate (step 1))
    print (negate (opaque (-9)))
    print (negate (-(opaque 11)))
    print (n - (-(-6)))
    print (m - (-(-0.5)))
    print (subNeg (negate (-5)) n)
    putStrLn "negate_inlined_negative_literal: done"
