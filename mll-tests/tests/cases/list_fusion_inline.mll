-- Fused pipelines emit small stage/fold BODIES in place of the
-- per-element calls (where-local and module inline candidates, cheap
-- lambdas and sections). The interesting pins are the decliners: a
-- lambda capturing a site local stays a closure call; a stage function
-- whose body reads a module-level name must NOT inline where a local
-- shadows that name (the sum would silently change — the free-variable
-- gate declines and the closure call resolves the module value
-- correctly); a locally shadowed Prelude name means the local function.
-- Also pinned: Prelude odd/not (odd is a direct definition now, and
-- saturated `not` emits natively), and rem's truncated semantics on
-- negative elements inside a fused loop.

module Main where

import Data.List (foldl')

-- Computed, so constant folding cannot erase the body's free variable —
-- the shadow pin below needs `shifted` to really read a module name.
offset :: Int
offset = sum [1 .. 13]

shifted :: Int -> Int
shifted x = x + offset

main :: IO ()
main = do
    -- Where-bound fold + predicate bodies inline; byte-parity is the pin.
    print (foldl' step 0 (filter big (map (* 3) [1 .. 2000 :: Int])))
    -- Prelude odd (module candidate through its specialization).
    print (foldl' step 0 (filter odd (map (+ 7) [1 .. 2000 :: Int])))
    -- Saturated not over a comparison, and first-class not (no args).
    print (not (3 > (5 :: Int)))
    print (map not [True, False])
    -- A lambda capturing a site local: declines, closure call, shared
    -- once-evaluated capture.
    let cap = sum [1 .. 10 :: Int]
    print (foldl' (+) 0 (map (\x -> x + cap) [1 .. 5 :: Int]))
    -- The shadow trap: `shifted`'s body reads module `offset`, and this
    -- do-block binds a LOCAL offset. Inlining the body here would read
    -- the local; the free-variable gate must keep the closure call.
    let offset = 5
    print (foldl' (+) 0 (map shifted [1 .. 10 :: Int]))
    print offset
    -- A locally shadowed Prelude predicate is the LOCAL function.
    let even x = x > (1990 :: Int)
    print (length (filter even [1 .. 2000 :: Int]))
    -- rem keeps truncated (GHC) semantics on negative fused elements.
    print (foldl' step 0 (filter negOdd (map (\v -> v - 20) [1 .. 40 :: Int])))
  where
    step a x = (a + x) `mod` 1000000007
    big x = x `rem` 2 /= 0
    negOdd x = x `rem` 2 /= 0
