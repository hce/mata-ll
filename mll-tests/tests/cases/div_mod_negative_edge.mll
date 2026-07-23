-- Regression companion to div_mod_fold_runtime_agree.mll: edge and
-- larger operands for floor-semantics `div`/`mod`, plus the algebraic
-- laws that must hold for ANY internally consistent div/mod pair and
-- the floor-specific properties that distinguish Haskell's semantics
-- from the buggy Euclidean fold.
--
-- Cases where the Euclidean fold DISAGREED with the floor runtime are
-- marked (splits). Cases where both semantics agree still matter: they
-- pin the folded value to the correct answer so a "fix" that breaks
-- the agreeing quadrants would be caught.
--
-- Note: mata-ll has no `quot`/`rem`/`divMod`/`quotRem` (checked
-- lib/Prelude.mll, lib/LMath.mll, mllc/src), so the truncating
-- counterparts cannot be tested. If they are ever added, add cases
-- pinning 7 `quot` (-2) == -3 and 7 `rem` (-2) == 1, distinct from
-- div/mod.

dDiv :: Int -> Int -> Int
dDiv a b = a `div` b

dMod :: Int -> Int -> Int
dMod a b = a `mod` b

-- Law: (a `div` b) * b + (a `mod` b) == a, evaluated at runtime.
-- (This law holds under Euclidean semantics too, so it does not detect
-- the fold/runtime split by itself -- it guards the runtime pair being
-- mutually consistent.)
divModIdentity :: Int -> Int -> Bool
divModIdentity a b = (a `div` b) * b + (a `mod` b) == a

-- Floor-specific law: the remainder lies in [0, b) for b > 0 and in
-- (b, 0] for b < 0 -- i.e. it takes the divisor's sign. The Euclidean
-- remainder violates the b < 0 half.
modRangeOk :: Int -> Int -> Bool
modRangeOk a b =
    let r = a `mod` b
    in if b > 0 then r >= 0 && r < b
                else r <= 0 && r > b

test_hundred_by_seven :: IO ()
test_hundred_by_seven = do
    -- floor: 100/7 family
    assert ((100 `div` 7) == 14)           "lit: 100 div 7 == 14"
    assert ((100 `mod` 7) == 2)            "lit: 100 mod 7 == 2"
    assert (((-100) `div` 7) == (-15))     "lit: -100 div 7 == -15"
    assert (((-100) `mod` 7) == 5)         "lit: -100 mod 7 == 5"
    assert ((100 `div` (-7)) == (-15))     "lit: 100 div -7 == -15 (splits)"
    assert ((100 `mod` (-7)) == (-5))      "lit: 100 mod -7 == -5 (splits)"
    assert (((-100) `div` (-7)) == 14)     "lit: -100 div -7 == 14 (splits)"
    assert (((-100) `mod` (-7)) == (-2))   "lit: -100 mod -7 == -2 (splits)"
    -- same, runtime path, plus agreement
    assert (dDiv 100 (-7) == (-15))        "run: 100 div -7 == -15"
    assert (dMod 100 (-7) == (-5))         "run: 100 mod -7 == -5"
    assert (dDiv (-100) (-7) == 14)        "run: -100 div -7 == 14"
    assert (dMod (-100) (-7) == (-2))      "run: -100 mod -7 == -2"
    assert ((100 `div` (-7)) == dDiv 100 (-7))     "agree: 100 div -7"
    assert ((100 `mod` (-7)) == dMod 100 (-7))     "agree: 100 mod -7"
    assert (((-100) `div` (-7)) == dDiv (-100) (-7)) "agree: -100 div -7"
    assert (((-100) `mod` (-7)) == dMod (-100) (-7)) "agree: -100 mod -7"

-- |a| < |b|: floor rounds these to -1 or 0; Euclid disagreed on two.
test_small_dividend :: IO ()
test_small_dividend = do
    assert ((1 `div` 2) == 0)              "lit: 1 div 2 == 0"
    assert ((1 `mod` 2) == 1)              "lit: 1 mod 2 == 1"
    assert (((-1) `div` 2) == (-1))        "lit: -1 div 2 == -1"
    assert (((-1) `mod` 2) == 1)           "lit: -1 mod 2 == 1"
    assert ((1 `div` (-2)) == (-1))        "lit: 1 div -2 == -1 (splits: Euclid 0)"
    assert ((1 `mod` (-2)) == (-1))        "lit: 1 mod -2 == -1 (splits: Euclid 1)"
    assert (((-1) `div` (-2)) == 0)        "lit: -1 div -2 == 0 (splits: Euclid 1)"
    assert (((-1) `mod` (-2)) == (-1))     "lit: -1 mod -2 == -1 (splits: Euclid 1)"
    assert (dDiv 1 (-2) == (-1))           "run: 1 div -2 == -1"
    assert (dMod 1 (-2) == (-1))           "run: 1 mod -2 == -1"
    assert (dDiv (-1) (-2) == 0)           "run: -1 div -2 == 0"
    assert (dMod (-1) (-2) == (-1))        "run: -1 mod -2 == -1"
    assert ((1 `div` (-2)) == dDiv 1 (-2))       "agree: 1 div -2"
    assert (((-1) `div` (-2)) == dDiv (-1) (-2)) "agree: -1 div -2"
    assert ((1 `mod` (-2)) == dMod 1 (-2))       "agree: 1 mod -2"
    assert (((-1) `mod` (-2)) == dMod (-1) (-2)) "agree: -1 mod -2"

-- Exact division: remainder must be exactly 0 in every quadrant
-- (both semantics agree here -- pins the fixed folder to correctness).
test_exact_division :: IO ()
test_exact_division = do
    assert (((-6) `div` 3) == (-2))        "lit: -6 div 3 == -2"
    assert (((-6) `mod` 3) == 0)           "lit: -6 mod 3 == 0"
    assert ((6 `div` (-3)) == (-2))        "lit: 6 div -3 == -2"
    assert ((6 `mod` (-3)) == 0)           "lit: 6 mod -3 == 0"
    assert (((-6) `div` (-3)) == 2)        "lit: -6 div -3 == 2"
    assert (((-6) `mod` (-3)) == 0)        "lit: -6 mod -3 == 0"
    assert (dDiv 6 (-3) == (-2))           "run: 6 div -3 == -2"
    assert (dMod 6 (-3) == 0)              "run: 6 mod -3 == 0"
    assert ((6 `div` (-3)) == dDiv 6 (-3)) "agree: 6 div -3"

-- Zero dividend and unit divisors.
test_zero_and_units :: IO ()
test_zero_and_units = do
    assert ((0 `div` (-5)) == 0)           "lit: 0 div -5 == 0"
    assert ((0 `mod` (-5)) == 0)           "lit: 0 mod -5 == 0"
    assert ((7 `div` (-1)) == (-7))        "lit: 7 div -1 == -7"
    assert ((7 `mod` (-1)) == 0)           "lit: 7 mod -1 == 0"
    assert (((-7) `div` (-1)) == 7)        "lit: -7 div -1 == 7"
    assert (((-7) `mod` (-1)) == 0)        "lit: -7 mod -1 == 0"
    assert (((-7) `div` 1) == (-7))        "lit: -7 div 1 == -7"
    assert (((-7) `mod` 1) == 0)           "lit: -7 mod 1 == 0"
    assert (dDiv 0 (-5) == (0 `div` (-5))) "agree: 0 div -5"
    assert (dDiv 7 (-1) == (7 `div` (-1))) "agree: 7 div -1"
    assert (dMod 7 (-1) == (7 `mod` (-1))) "agree: 7 mod -1"

-- Larger operands (well inside double-exact integer range).
test_large_operands :: IO ()
test_large_operands = do
    assert (((-123456789) `div` 1000) == (-123457)) "lit: -123456789 div 1000"
    assert (((-123456789) `mod` 1000) == 211)       "lit: -123456789 mod 1000"
    assert ((123456789 `div` (-1000)) == (-123457)) "lit: 123456789 div -1000 (splits)"
    assert ((123456789 `mod` (-1000)) == (-211))    "lit: 123456789 mod -1000 (splits)"
    assert (dDiv (-123456789) 1000 == (-123457))    "run: -123456789 div 1000"
    assert (dMod (-123456789) 1000 == 211)          "run: -123456789 mod 1000"
    assert (dDiv 123456789 (-1000) == (-123457))    "run: 123456789 div -1000"
    assert (dMod 123456789 (-1000) == (-211))       "run: 123456789 mod -1000"
    assert ((123456789 `div` (-1000)) == dDiv 123456789 (-1000)) "agree: big div"
    assert ((123456789 `mod` (-1000)) == dMod 123456789 (-1000)) "agree: big mod"

test_identity_law :: IO ()
test_identity_law = do
    assert (divModIdentity 7 2)            "identity: 7 2"
    assert (divModIdentity (-7) 2)         "identity: -7 2"
    assert (divModIdentity 7 (-2))         "identity: 7 -2"
    assert (divModIdentity (-7) (-2))      "identity: -7 -2"
    assert (divModIdentity 100 (-7))       "identity: 100 -7"
    assert (divModIdentity (-100) (-7))    "identity: -100 -7"
    assert (divModIdentity 1 (-2))         "identity: 1 -2"
    assert (divModIdentity (-1) (-2))      "identity: -1 -2"
    assert (divModIdentity (-123456789) 1000)  "identity: big pos divisor"
    assert (divModIdentity 123456789 (-1000))  "identity: big neg divisor"
    -- Fully-literal identity: exercises the identity through the
    -- FOLDER (every subterm is foldable). Holds under Euclid too, so
    -- this alone can't detect the split -- but combined with the
    -- pinned div values above it pins the folded mod values.
    assert (((7 `div` (-2)) * (-2) + (7 `mod` (-2))) == 7)       "lit identity: 7 -2"
    assert ((((-7) `div` (-2)) * (-2) + ((-7) `mod` (-2))) == (-7)) "lit identity: -7 -2"

test_mod_range_law :: IO ()
test_mod_range_law = do
    assert (modRangeOk 7 2)                "mod range: 7 2"
    assert (modRangeOk (-7) 2)             "mod range: -7 2"
    assert (modRangeOk 7 (-2))             "mod range: 7 -2 (sign of divisor)"
    assert (modRangeOk (-7) (-2))          "mod range: -7 -2 (sign of divisor)"
    assert (modRangeOk 100 (-7))           "mod range: 100 -7"
    assert (modRangeOk (-100) (-7))        "mod range: -100 -7"
    assert (modRangeOk (-123456789) 1000)  "mod range: big pos divisor"
    assert (modRangeOk 123456789 (-1000))  "mod range: big neg divisor"

main :: IO ()
main = do
    test_hundred_by_seven
    test_small_dividend
    test_exact_division
    test_zero_and_units
    test_large_operands
    test_identity_law
    test_mod_range_law
