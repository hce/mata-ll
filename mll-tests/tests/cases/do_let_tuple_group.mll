-- Test: a do-block `let` with a TUPLE pattern is one binding group with
-- the bindings that follow it at the same column — the same layout and
-- letrec rules as a let-expression. The do-block used to parse
-- `let (a, b) = e` on a separate path that neither opened the group's
-- layout column nor joined the sibling loop, so a following `c = a + b`
-- was swallowed as continuation arguments of `pair` ("Expected
-- expression, found '='").

pair :: (Int, Int)
pair = (3, 4)

main :: IO ()
main = do
    let (a, b) = pair
        c = a + b
        double x = x * 2
        (p, q) = (double c, c - 1)
    putStrLn (show a)
    putStrLn (show b)
    putStrLn (show c)
    putStrLn (show p)
    putStrLn (show q)
    -- A tuple binding whose right-hand side refers to a LATER sibling of
    -- the same group (Haskell 2010 letrec), and one whose pattern is only
    -- matched on demand.
    let (m, n) = (k + 1, k + 2)
        k = 10
        (_, lazyErr) = (0 :: Int, error "never forced" :: Int)
    putStrLn (show (m + n))
    putStrLn (show (fst (1 :: Int, lazyErr)))
    -- The statement after a `let` group, at the `let` line's own indent,
    -- is a statement — not a binding.
    let z = 5
    putStrLn (show (z + 1))
-- expect: 3
-- expect: 4
-- expect: 7
-- expect: 14
-- expect: 6
-- expect: 23
-- expect: 1
-- expect: 6
