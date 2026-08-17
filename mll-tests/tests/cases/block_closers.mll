-- Test: an implicit `do` / `case` block ends at a token that cannot
-- continue it — the `,` of an enclosing tuple or list, `]`, `then`/`else`
-- of an enclosing `if`, `in` of an enclosing `let`, a `where` at the
-- statements' own indent — not only at `)` and at a dedent. (Haskell's
-- layout algorithm: the parse-error(t) rule.) `(do …, 2)` used to fail
-- with "Expected expression, found ','".

firstOfPair :: (IO Int, Int)
firstOfPair = (do let x = 20
                  return (x + 1), 2)

acts :: [IO Int]
acts = [do return 1, do let y = 2
                        return y, return 3]

pick :: Bool -> IO Int
pick c = if c then do return 10 else do return 20

viaLet :: IO Int
viaLet = let a = do return 7 in a

classified :: Int -> Int
classified n = fst (case n of
    0 -> 100
    _ -> 200, n)

withWhere :: IO Int
withWhere = do
    let r = helper + 1
    return r
    where
      helper = 41

main :: IO ()
main = do
    v <- fst firstOfPair
    assert (v == 21 && snd firstOfPair == 2) "do as first tuple element closes at ,"
    xs <- sequence acts
    assert (xs == [1, 2, 3]) "do blocks as list elements close at , and ]"
    a <- pick True
    b <- pick False
    assert (a == 10 && b == 20) "one-line if/then do/else do"
    l <- viaLet
    assert (l == 7) "do closed by `in`"
    assert (classified 0 == 100 && classified 5 == 200) "case alternatives close at ,"
    w <- withWhere
    assert (w == 42) "where at the statements' indent closes the do block"
