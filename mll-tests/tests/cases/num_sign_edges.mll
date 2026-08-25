-- Signed-zero and NaN edges of abs/signum at Number (GHC.Float
-- parity).  Regression: `abs (-0.0)` returned -0.0 (the `x < 0` test
-- let it through), and `signum NaN` returned 0.0 where GHC's guard
-- definition (x > 0 -> 1, x < 0 -> -1, otherwise -> x itself) makes
-- it NaN — and signum (-0.0) is -0.0.  (The round-3 finding claimed
-- GHC gives -1.0 for NaN; runghc refuted that.)

main :: IO ()
main = do
    print (abs (-0.0))
    print (abs (0.0 :: Number))
    print (abs (-2.5))
    print (signum (0/0 :: Number))
    print (signum (-3.5))
    print (signum (0.0 :: Number))
    print (signum (-0.0 :: Number))
