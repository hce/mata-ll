-- Integer division on multi-limb operands: the schoolbook (Knuth D)
-- divide replaced the bit-by-bit binary loop, and this matrix walks its
-- paths — one-limb short division, top-heavy divisors whose two-limb
-- qhat estimate clamps at B-1, divisors with a minimal top limb (heavy
-- normalization), the add-back shape (2^95 over 2^71 + 1), exact
-- multiples and their +-1 neighbors — over all four sign combinations.
-- divMod/quotRem laws and remainder-sign laws are asserted per pair so
-- a failure names the operands; everything folds into one checksum and
-- two full decimal quotients pinned byte-exact against GHC (GMP).

module Main where

import Data.List (foldl')

pow :: Integer -> Int -> Integer
pow _ 0 = 1
pow b n = b * pow b (n - 1)

fact :: Int -> Integer
fact 0 = 1
fact n = toInteger n * fact (n - 1)

dividends :: [Integer]
dividends =
    [ pow 2 95                                        -- add-back trigger vs 2^71 + 1
    , pow 10 120 + 12345
    , fact 60
    , pow 2 400 - 1
    , pow 16777216 20 + pow 16777216 10 + 1           -- sparse limbs
    , (pow 2 300 + 7) * (pow 10 40 + 9) + 123456789   -- exact multiple + offset
    ]

divisors :: [Integer]
divisors =
    [ 3                                               -- one-limb, tiny
    , 16777213                                        -- one-limb, near 2^24
    , pow 2 71 + 1                                    -- add-back pair for 2^95
    , pow 2 96 - 1                                    -- every limb B-1 (qhat clamp)
    , pow 2 72 + 5                                    -- top limb 1 (normalization)
    , pow 10 40 + 9
    , fact 30
    ]

p :: Integer
p = 1000000007

law :: Integer -> Integer -> Integer -> Integer
law acc a b =
    let (q, r)   = a `divMod` b
        (q', r') = a `quotRem` b
    in if a /= q * b + r || a /= q' * b + r'
           then error ("division law broken: " <> show a <> " / " <> show b)
           else if r /= 0 && signum r /= signum b
               then error ("mod sign broken: " <> show a <> " / " <> show b)
               else if r' /= 0 && signum r' /= signum a
                   then error ("rem sign broken: " <> show a <> " / " <> show b)
                   else (((acc * 31 + q) * 31 + r) * 31 + q' + r') `mod` p

main :: IO ()
main = do
    let acc = foldl' (\z (a, b) -> law z a b) 0
            [ (sa * a, sb * b) | a <- dividends, b <- divisors, sa <- [1, -1], sb <- [1, -1] ]
    print acc
    -- Two quotients printed in full: decimal conversion of big results is
    -- itself repeated division, so these pin digits, not just a residue.
    print (pow 2 95 `div` (pow 2 71 + 1))
    print ((pow 10 120 + 12345) `divMod` (pow 10 40 + 9))
