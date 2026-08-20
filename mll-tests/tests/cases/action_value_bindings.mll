-- Action-typed VALUE bindings must be re-performable, not memoized.
-- Regression: where-bound and expression-let-bound actions were emitted as
-- memoizing thunks — `w >> w where w = putStrLn "hi"` performed the effect
-- on the first force, cached the unit result, and printed once. GHC's
-- semantics: an IO action value is a description; every use performs it.
-- The do-block `let` path already emitted a re-performable closure; the
-- where and expression-let paths now mirror it (and this case pins all
-- three).

top :: IO ()
top = putStrLn "top"

-- where-bound actions: a computed one and a bare-var alias of a top-level
fromWhere :: IO ()
fromWhere = w >> w >> a >> a
  where
    w = putStrLn "where"
    a = top

-- expression-position let: the let sits in argument position, so its
-- bindings go through the value emitter, not the do-block chain
run2 :: IO () -> IO ()
run2 x = x >> x

fromLet :: IO ()
fromLet = run2 (let u = putStrLn "let-expr" in u)

main :: IO ()
main = do
    fromWhere
    fromLet
    -- do-let (the reference path): pinned so the three stay consistent
    let d = putStrLn "do-let"
    d
    d
