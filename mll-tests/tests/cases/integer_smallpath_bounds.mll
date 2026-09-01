-- Integer arithmetic across the small-magnitude fast-path boundaries.
-- The runtime computes add/sub/mul/divmod natively when both operands
-- fit two limbs (< 2^48) and falls back to limb arithmetic above that;
-- this matrix straddles every boundary the fast paths key on (limb
-- edges 2^24 and 2^48, the mul exactness bound near 2^52..2^53) with
-- both signs, and folds every divMod/quotRem/add/sub/mul result into
-- one checksum. The GHC golden (GMP arithmetic) pins byte-exact
-- results; the division laws are also asserted directly so a failure
-- names the offending pair instead of just moving the checksum.

module Main where

vals :: [Integer]
vals = base ++ map negate base
  where
    base = [ 0, 1, 2, 7, 100
           , 16777215, 16777216, 16777217              -- one-limb edge (2^24)
           , 281474976710655, 281474976710656          -- two-limb edge (2^48)
           , 281474976710657
           , 4503599627370495, 4503599627370496        -- mul bound target (2^52)
           , 9007199254740991, 9007199254740993        -- double-exactness edge
           , 1152921504606846976                       -- 2^60: slow path
           ]

p :: Integer
p = 1000000007

step :: Integer -> Integer -> Integer -> Integer
step acc a b =
    if b == 0
        then (acc * 31 + a * a - a) `mod` p
        else
            let (q, r)   = a `divMod` b
                (q', r') = a `quotRem` b
            in if a /= q * b + r
                   then error ("divMod law broken: " <> show a <> " " <> show b)
                   else if a /= q' * b + r'
                       then error ("quotRem law broken: " <> show a <> " " <> show b)
                       else if (a + b) - b /= a
                           then error ("add/sub law broken: " <> show a <> " " <> show b)
                           else ((acc * 31 + q + r) * 31 + q' + r' + a * b) `mod` p

main :: IO ()
main = do
    let acc = foldl (\z (a, b) -> step z a b) 0 [(a, b) | a <- vals, b <- vals]
    print acc

-- expect: 475521926
