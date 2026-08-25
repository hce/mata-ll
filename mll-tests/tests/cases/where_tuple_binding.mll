-- Tuple-pattern bindings in `where` blocks, with the let path's
-- lazy-selector desugar (fresh scrutinee + one selector per variable,
-- one recursive group).  Regression: parse_where broke out of its loop
-- on the '(' and the binding died far away as "Expected operator",
-- while the identical binding in a `let` worked.

f :: Int -> Int
f n = a + b
  where
    (a, b) = (n, n * 2)

-- selectors are lazy: the bottom component is never demanded
safeFst :: Int
safeFst = x
  where
    (x, y) = (7, error "never demanded")

-- pattern variables are in scope for SIBLING where bindings
chained :: Int -> Int
chained n = lo + hi + extra
  where
    (lo, hi) = (n, n + 1)
    extra = hi * 10

main :: IO ()
main = do
    print (f 5)
    print safeFst
    print (chained 2)
