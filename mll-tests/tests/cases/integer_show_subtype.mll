-- Integer limbs may carry Lua's float subtype on 5.3+ (carry
-- propagation divides with `/`, machine fromInteger walks with `/`),
-- and show must never leak it: a float-typed limb reaching a value
-- with a single decimal group printed "6.0" until __int_tostring
-- formatted the top group with %d. Both shapes that reached it are
-- pinned: a small-fast-path result whose operand had a float-typed
-- second limb (2^24 + 1 built from a machine literal), and a short-
-- division remainder inheriting a big dividend's float-typed limbs.

module Main where

main :: IO ()
main = do
    print ((16777217 - 16777211) :: Integer)
    print (957039950588242312856580 `mod` 9)
    print ((16777217 * 16777215) `div` 16777216 :: Integer)
    print (negate 957039950588242312856580 `rem` 1000003)
