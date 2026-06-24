-- Infix-LHS definitions: an operator or a backtick'd name may appear between
-- its two operands on the left of `=`, as in standard Haskell.

(|+|) :: Integer -> Integer -> Integer
a |+| b = a + b

-- Backtick'd function name with variable operands.
add3 :: Integer -> Integer -> Integer
x `add3` y = x + y + 3

-- Infix-LHS with guards in the body.
clampLo :: Integer -> Integer -> Integer
lo `clampLo` v
    | v < lo    = lo
    | otherwise = v

-- Multiple clauses, infix LHS, with a `where`.
(|*|) :: Integer -> Integer -> Integer
a |*| b = scaled
  where scaled = a * b

main :: IO ()
main = do
    assert (3 |+| 4 == 7) "operator infix-LHS def"
    assert (10 `add3` 5 == 18) "backtick infix-LHS def"
    assert (5 `clampLo` 2 == 5) "infix-LHS def with guard (lo)"
    assert (5 `clampLo` 9 == 9) "infix-LHS def with guard (otherwise)"
    assert (6 |*| 7 == 42) "infix-LHS def with where"
