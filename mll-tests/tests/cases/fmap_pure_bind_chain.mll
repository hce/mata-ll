-- Regression test for B5: fmap/<$> over pure/return in a do-block bind chain.
--
-- In a do-block with 2+ continuation statements, the typechecker uses the
-- iterative bind-chain path (infer_bind_chain). That path used to skip the
-- unification of each bind's result type with the type of its continuation,
-- so a bound expression that doesn't pin the monad by itself — like
-- `fmap f (pure x)`, where both fmap and pure are class methods — stayed
-- polymorphic. Monomorphization then emitted a reference to the bare class
-- method name (an undefined Lua global) and the program crashed at runtime
-- with "attempt to call a nil value".
--
-- These asserts intentionally sit in long do-blocks with NO type annotations
-- on the bound expressions: the monad must be inferred purely from the
-- do-block context flowing backwards along the chain.

main :: IO ()
main = do
    -- The original repro shape: fmap over pure, then 2+ more statements.
    x <- fmap (+1) (pure 1)
    assert (x == 2) "fmap over pure in >=2-statement chain"

    -- <$> operator form of the same bug.
    y <- (+1) <$> pure 1
    assert (y == 2) "<$> over pure in bind chain"

    -- return instead of pure.
    z <- fmap (+1) (return 1)
    assert (z == 2) "fmap over return in bind chain"

    -- Element-type-changing fmap: the functor var is the only link to IO.
    s <- fmap show (pure 5)
    assert (s == "5") "element-type-changing fmap over pure"

    -- 3-statement chain of polymorphic binds feeding each other: the monad
    -- is only determined by the very end of the chain.
    a <- fmap (+1) (pure 10)
    b <- fmap (* 2) (pure a)
    c <- (+ 100) <$> return b
    assert (c == 122) "3-statement chain of fmap/<$> over pure/return"

    -- A do-let between the bind and the statements that pin the monad must
    -- stay transparent to the backward propagation.
    w <- fmap (+1) (pure 5)
    let doubled = w * 2
    assert (doubled == 12) "let statement between bind and continuation"

    -- Guards: fmap over lists and Maybe must keep their own instances even
    -- inside an IO bind chain.
    assert (fmap (+1) [1, 2, 3] == [2, 3, 4]) "fmap list unaffected"
    assert (fmap (+1) (Just 41) == Just 42) "fmap Maybe unaffected"
    assert (((+1) <$> Just 41) == Just 42) "<$> Maybe unaffected"

    -- Guard: fmap over a concretely-IO action (worked before the fix too).
    args <- fmap length getArgs
    assert (args == 0) "fmap over concrete IO action"

    shortDo
    putStrLn "ok"

-- Guard: single-continuation do-block (the non-chain typechecking path).
-- Kept in a separate function so the do-block really has only 2 statements.
shortDo :: IO ()
shortDo = do
    x <- fmap (+1) (pure 1)
    assert (x == 2) "fmap over pure in single-continuation do"
