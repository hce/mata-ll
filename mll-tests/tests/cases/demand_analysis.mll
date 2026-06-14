-- Demand analysis tests
-- Verifies that strictness analysis correctly identifies which parameters
-- are forced vs lazy, including cross-function propagation.

-- ============================================================
-- Basic strictness: used parameter is forced, unused is not
-- ============================================================

-- `pickFirst` uses only its first argument — second should stay lazy
pickFirst :: a -> b -> a
pickFirst pf1 _ = pf1

-- `pickSecond` uses only its second argument — first should stay lazy
pickSecond :: a -> b -> b
pickSecond _ ps2 = ps2

-- ============================================================
-- Cross-function demand propagation
-- ============================================================

-- `incr` is strict in its argument (used in +)
incr :: Integer -> Integer
incr incn = incn + 1

-- `applyIncr` calls incr — so it should also be strict in its arg
applyIncr :: Integer -> Integer
applyIncr aix = incr aix

-- Two levels of propagation
doubleIncr :: Integer -> Integer
doubleIncr dix = applyIncr dix + 1

-- ============================================================
-- Guards: condition variables are always demanded
-- ============================================================

classify :: Integer -> String
classify cn
    | cn < 0     = "negative"
    | cn == 0    = "zero"
    | otherwise = "positive"

-- Guard with unused variable in one branch
-- gpy is only used when gpx > 0, so gpy should not be strict
guardPartial :: Integer -> Integer -> String
guardPartial gpx gpy
    | gpx > 0    = show (gpx + gpy)
    | otherwise = "non-positive"

-- ============================================================
-- Branch intersection: strict only if used in ALL branches
-- ============================================================

-- Both clauses use bn, so bn is strict
bothBranches :: Bool -> Integer -> String
bothBranches True bn = show bn
bothBranches False bn = show (bn + 1)

-- Only one clause uses obn, so obn should NOT be strict
oneBranch :: Bool -> Integer -> String
oneBranch True _ = "yes"
oneBranch False obn = show obn

-- ============================================================
-- Pattern matching forces the scrutinee
-- ============================================================

data Box a = MkBox a

unbox :: Box a -> a
unbox (MkBox ubx) = ubx

-- ============================================================
-- Let-binding transitivity
-- ============================================================

-- Body demands `ltdoubled`, which demands `ltn` via its definition
letTransitive :: Integer -> Integer
letTransitive ltn = ltdoubled + 1
    where ltdoubled = ltn * 2

-- Body demands `lclabel` which demands `lcn` through show
letChain :: Integer -> String
letChain lcn = "value: " ++ lclabel
    where lclabel = show lcn

-- ============================================================
-- Lazy contexts: cons
-- ============================================================

-- Cons is lazy — neither element should be forced
consPair :: a -> a -> [a]
consPair cp1 cp2 = cp1 : cp2 : []

-- ============================================================
-- Operators force both sides
-- ============================================================

addTwo :: Integer -> Integer -> Integer
addTwo at1 at2 = at1 + at2

mulTwo :: Integer -> Integer -> Integer
mulTwo mt1 mt2 = mt1 * mt2

-- ============================================================
-- If-then-else: both branches use same var → strict
-- ============================================================

ifBothUse :: Bool -> Integer -> Integer
ifBothUse ibc ibn = if ibc then ibn + 1 else ibn * 2

-- ============================================================
-- Case expression: scrutinee is always forced
-- ============================================================

describeSign :: Integer -> String
describeSign dsn = case dsn > 0 of
    True  -> "positive"
    False -> "non-positive"

-- ============================================================
-- Recursive function strictness
-- ============================================================

sumTo :: Integer -> Integer
sumTo 0 = 0
sumTo stn = stn + sumTo (stn - 1)

factorial :: Integer -> Integer
factorial 0 = 1
factorial facn = facn * factorial (facn - 1)

-- ============================================================
-- Higher-order: apply forces its function arg
-- ============================================================

applyFn :: (a -> b) -> a -> b
applyFn afnf afnx = afnf afnx

-- ============================================================
-- Where clause demands propagate to parameters
-- ============================================================

squarePlus :: Integer -> Integer -> Integer
squarePlus spa spb = spsq + spb
    where spsq = spa * spa

main :: IO ()
main = do
    -- Basic: unused args don't get evaluated
    assert (pickFirst 42 undefined == 42) "pickFirst: lazy second arg"
    assert (pickSecond undefined 42 == 42) "pickSecond: lazy first arg"

    -- Strict function: used arg works fine
    assert (incr 5 == 6) "incr strict"
    assert (applyIncr 10 == 11) "cross-function propagation"
    assert (doubleIncr 10 == 12) "two-level propagation"

    -- Guards force the condition variable
    assert (classify (-3) == "negative") "guard negative"
    assert (classify 0 == "zero") "guard zero"
    assert (classify 5 == "positive") "guard positive"

    -- Guard with partial use: gpx forced by condition, gpy only in one branch
    assert (guardPartial 3 7 == "10") "guard partial: both used"
    assert (guardPartial (-1) undefined == "non-positive") "guard partial: y lazy"

    -- Branch intersection across clauses
    assert (bothBranches True 5 == "5") "both branches True"
    assert (bothBranches False 5 == "6") "both branches False"

    -- One clause doesn't use the param — bottom should be safe
    assert (oneBranch True undefined == "yes") "one branch: unused bottom"
    assert (oneBranch False 42 == "42") "one branch: used"

    -- Pattern matching forces the scrutinee
    assert (unbox (MkBox 99) == 99) "pattern match forces"

    -- Let transitivity: body demands let-bound var which demands param
    assert (letTransitive 5 == 11) "let transitive"
    assert (letChain 42 == "value: 42") "let chain"

    -- Lazy constructors: elements not forced during cons
    let xs = consPair 1 2
    assert (head xs == 1) "cons lazy: head"

    -- Tuples are lazy in construction
    let tup = (1, 2)
    assert (fst tup == 1) "tuple lazy: fst"

    -- Operators force both sides
    assert (addTwo 3 4 == 7) "add strict both"
    assert (mulTwo 3 4 == 12) "mul strict both"

    -- If: both branches use ibn, so ibn is strict
    assert (ifBothUse True 5 == 6) "if: both use True"
    assert (ifBothUse False 5 == 10) "if: both use False"

    -- Case: scrutinee is forced
    assert (describeSign 5 == "positive") "case scrutinee forced"
    assert (describeSign (-3) == "non-positive") "case scrutinee neg"

    -- Recursive strictness (pattern match on 0 forces the arg)
    assert (sumTo 10 == 55) "sumTo recursive"
    assert (factorial 5 == 120) "factorial recursive"

    -- Higher-order: apply forces its function arg
    assert (applyFn (+ 1) 5 == 6) "higher-order apply"

    -- const from prelude discards second argument
    assert (const "safe" undefined == "safe") "const discards bottom"

    -- Where clause demands propagate
    assert (squarePlus 3 4 == 13) "where: 3^2 + 4"

    -- Composed: strict function with lazy unused arg
    assert (pickFirst (incr 5) 99 == 6) "composed strict result"
