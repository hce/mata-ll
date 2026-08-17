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
    assert (a == 3 && b == 4) "tuple binding"
    assert (c == 7) "sibling simple binding sees the tuple's variables"
    assert (p == 14 && q == 6) "sibling function + second tuple binding"
    -- A tuple binding whose right-hand side refers to a LATER sibling of
    -- the same group (Haskell 2010 letrec), and one whose pattern is only
    -- matched on demand.
    let (m, n) = (k + 1, k + 2)
        k = 10
        (_, lazyErr) = (0 :: Int, error "never forced" :: Int)
    assert (m + n == 23) "tuple RHS refers to a later sibling (letrec)"
    assert (fst (1 :: Int, lazyErr) == 1) "unforced selector stays lazy"
    -- The statement after a `let` group, at the `let` line's own indent,
    -- is a statement — not a binding.
    let z = 5
    assert (z + 1 == 6) "statement after the group"
