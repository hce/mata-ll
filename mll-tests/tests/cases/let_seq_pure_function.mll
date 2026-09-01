-- The strict-accumulator let lowering (`let z = RHS in seq z REST`)
-- must NOT fire when the let is action-typed: `seq z (pure z)`
-- terminates a bind chain, and lowering its `pure` through the plain
-- expression walk would lose the __mll_pure box — a returned
-- `pure <function>` would then be mistaken for an action closure and
-- invoked by the runner. Surfaced by a tracker.lua codegen diff; the
-- function-valued round trip below is the case that would miscompile.

module Main where

mkAdd :: Int -> (Int -> Int)
mkAdd n = \x -> x + n

getF :: Int -> IO (Int -> Int)
getF n = let f = mkAdd n in f `seq` pure f

main :: IO ()
main = do
    f <- getF 5
    print (f 10)
    -- the pure (non-action) shape of the same lowering, for contrast:
    -- eagerly-bound accumulator, seq consumed at the binding
    let r = let z = f 20 + 1 in z `seq` (z * 2)
    print r

-- expect: 15
-- expect: 52
