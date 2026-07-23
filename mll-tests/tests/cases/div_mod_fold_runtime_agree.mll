-- Regression: constant folder and runtime must agree on `div`/`mod` with
-- negative operands, and both must implement Haskell's FLOOR semantics.
--
-- Background: the folder (mllc/src/fold.rs, fold_int_int) evaluated
-- literal `div`/`mod` with Rust's div_euclid/rem_euclid (Euclidean),
-- while the runtime emits Lua `math.floor(a / b)` and `%` (floor,
-- = Haskell). So `7 `div` (-2)` folded to -3 at compile time but
-- computed -4 at runtime. This file pins every sign combination three
-- ways per case:
--   lit:   both operands are literals  -> the folder evaluates it
--   run:   operands flow through function parameters -> the folder
--          cannot see them, so the runtime evaluates it
--   agree: lit form == run form (the unsoundness itself)
--
-- Correct Haskell (floor) answers:
--   div:  7`div`2 == 3    (-7)`div`2 == -4   7`div`(-2) == -4   (-7)`div`(-2) == 3
--   mod:  7`mod`2 == 1    (-7)`mod`2 == 1    7`mod`(-2) == -1   (-7)`mod`(-2) == -1
-- (mod takes the sign of the DIVISOR; the buggy Euclidean fold made it
-- always non-negative, e.g. 7`mod`(-2) folded to 1 instead of -1.)

-- Runtime path: the folder only folds InfixApp with literal operands,
-- so `a` and `b` here are opaque to it.
dDiv :: Int -> Int -> Int
dDiv a b = a `div` b

dMod :: Int -> Int -> Int
dMod a b = a `mod` b

-- Extra-opaque runtime path: operands read back out of a list, so even
-- a future const-propagation through trivial wrappers won't fold this.
opaque :: Int -> Int
opaque n = head [n]

-- Half-literal path: one literal, one variable. Never folded (the
-- folder needs both sides literal), but exercises the direct InfixApp
-- codegen for div/mod rather than the call/inline path.
negTwo :: Int
negTwo = -2

posTwo :: Int
posTwo = 2

test_div_lit :: IO ()
test_div_lit = do
    assert ((7 `div` 2) == 3)          "lit div: 7 div 2 == 3"
    assert (((-7) `div` 2) == (-4))    "lit div: -7 div 2 == -4"
    assert ((7 `div` (-2)) == (-4))    "lit div: 7 div -2 == -4 (folder gave -3)"
    assert (((-7) `div` (-2)) == 3)    "lit div: -7 div -2 == 3 (folder gave 4)"

test_div_run :: IO ()
test_div_run = do
    assert (dDiv 7 2 == 3)             "run div: 7 div 2 == 3"
    assert (dDiv (-7) 2 == (-4))       "run div: -7 div 2 == -4"
    assert (dDiv 7 (-2) == (-4))       "run div: 7 div -2 == -4"
    assert (dDiv (-7) (-2) == 3)       "run div: -7 div -2 == 3"
    assert (opaque 7 `div` opaque (-2) == (-4))    "opaque div: 7 div -2 == -4"
    assert (opaque (-7) `div` opaque (-2) == 3)    "opaque div: -7 div -2 == 3"

test_div_agree :: IO ()
test_div_agree = do
    assert ((7 `div` 2) == dDiv 7 2)             "agree div: 7 2"
    assert (((-7) `div` 2) == dDiv (-7) 2)       "agree div: -7 2"
    assert ((7 `div` (-2)) == dDiv 7 (-2))       "agree div: 7 -2 (fold/runtime split)"
    assert (((-7) `div` (-2)) == dDiv (-7) (-2)) "agree div: -7 -2 (fold/runtime split)"

test_mod_lit :: IO ()
test_mod_lit = do
    assert ((7 `mod` 2) == 1)          "lit mod: 7 mod 2 == 1"
    assert (((-7) `mod` 2) == 1)       "lit mod: -7 mod 2 == 1"
    assert ((7 `mod` (-2)) == (-1))    "lit mod: 7 mod -2 == -1 (folder gave 1)"
    assert (((-7) `mod` (-2)) == (-1)) "lit mod: -7 mod -2 == -1 (folder gave 1)"

test_mod_run :: IO ()
test_mod_run = do
    assert (dMod 7 2 == 1)             "run mod: 7 mod 2 == 1"
    assert (dMod (-7) 2 == 1)          "run mod: -7 mod 2 == 1"
    assert (dMod 7 (-2) == (-1))       "run mod: 7 mod -2 == -1"
    assert (dMod (-7) (-2) == (-1))    "run mod: -7 mod -2 == -1"
    assert (opaque 7 `mod` opaque (-2) == (-1))    "opaque mod: 7 mod -2 == -1"
    assert (opaque (-7) `mod` opaque (-2) == (-1)) "opaque mod: -7 mod -2 == -1"

test_mod_agree :: IO ()
test_mod_agree = do
    assert ((7 `mod` 2) == dMod 7 2)             "agree mod: 7 2"
    assert (((-7) `mod` 2) == dMod (-7) 2)       "agree mod: -7 2"
    assert ((7 `mod` (-2)) == dMod 7 (-2))       "agree mod: 7 -2 (fold/runtime split)"
    assert (((-7) `mod` (-2)) == dMod (-7) (-2)) "agree mod: -7 -2 (fold/runtime split)"

-- One operand literal, one a top-level variable: not foldable, must
-- still be floor semantics at runtime.
test_half_literal :: IO ()
test_half_literal = do
    assert ((7 `div` negTwo) == (-4))    "half-lit: 7 div negTwo == -4"
    assert ((7 `mod` negTwo) == (-1))    "half-lit: 7 mod negTwo == -1"
    assert (((-7) `div` posTwo) == (-4)) "half-lit: -7 div posTwo == -4"
    assert (((-7) `mod` posTwo) == 1)    "half-lit: -7 mod posTwo == 1"
    assert ((7 `div` negTwo) == (7 `div` (-2))) "half-lit agrees with lit: div"
    assert ((7 `mod` negTwo) == (7 `mod` (-2))) "half-lit agrees with lit: mod"

main :: IO ()
main = do
    test_div_lit
    test_div_run
    test_div_agree
    test_mod_lit
    test_mod_run
    test_mod_agree
    test_half_literal
