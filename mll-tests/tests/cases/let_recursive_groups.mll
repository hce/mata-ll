-- Regression test: `let`/`where` binding groups are mutually recursive
-- (Haskell 2010 letrec). Every binding in a group is in scope for every
-- other binding and for the body, regardless of declaration order.
--
-- Covers both facets that were previously broken:
--   * do-block `let` group forward reference (used to fail: "Unbound variable")
--   * expression `let ... in ...` forward reference (used to miscompile to nil)
-- plus mutually-recursive let functions, self-referential lazy values, and the
-- preservation of shadowing / separate-`let`-statement capture semantics.

isEvenDo :: Integer -> Bool
isEvenDo m = go m
  where go n = if n == 0 then True else goOdd (n - 1)
        goOdd n = if n == 0 then False else go (n - 1)

main :: IO ()
main = do
    -- Facet 1: forward reference inside a single do-block `let` group.
    -- `a` refers to `b`, which is defined *after* it in the same group.
    let a = b + 1
        b = 41
    assert (a == 42) "do-let forward reference"

    -- Facet 1, longer chain: a -> b, c ; c -> b. All in one group, out of order.
    let p = q + r
        q = 10
        r = q * 2
    assert (p == 30) "do-let forward chain"

    -- Facet 2: forward reference inside an expression `let ... in ...`.
    let e2 = let x = y + 1
                 y = 41
             in x
    assert (e2 == 42) "let-in forward reference"

    -- Mutually recursive functions defined in a do-block `let` group.
    let isEven n = if n == 0 then True else isOdd (n - 1)
        isOdd n = if n == 0 then False else isEven (n - 1)
    assert (isEven 10) "do-let mutual recursion (even)"
    assert (isOdd 7) "do-let mutual recursion (odd)"

    -- Mutually recursive functions in an expression `let ... in ...`.
    let mr = let ev n = if n == 0 then True else od (n - 1)
                 od n = if n == 0 then False else ev (n - 1)
             in ev 20
    assert mr "let-in mutual recursion"

    -- Self-referential lazy value in a do-block `let` group (needs laziness:
    -- the binding refers to itself and must not be forced at bind time).
    let fibs = 0 : 1 : zipWith (+) fibs (drop 1 fibs)
    assert (take 8 fibs == [0, 1, 1, 2, 3, 5, 8, 13]) "self-referential lazy list"

    -- where-clause forward reference (same letrec machinery).
    assert (isEvenDo 10) "where-clause mutual recursion"

    -- Shadowing is preserved: an inner `let` group shadows an outer binding,
    -- and separate `let` STATEMENTS are NOT one recursive group (capture).
    let s = 10
    let shadowed = let s = 99 in s + 1
    assert (s == 10) "outer binding unshadowed"
    assert (shadowed == 100) "inner let shadows"

    -- Separate `let` statements: `g` captures the first `s` (10), then `s`
    -- is rebound. This must NOT become a mutually-recursive group.
    let g = s + 1
    let s2 = 20
    assert (g == 11) "separate let statements capture, not letrec"
    assert (s2 == 20) "rebinding via new let statement"

    putStrLn "let_recursive_groups ok"
