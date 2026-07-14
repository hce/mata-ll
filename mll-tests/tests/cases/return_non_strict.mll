-- Regression (audit finding 6): `return` / `pure` must be NON-STRICT — a
-- returned bottom is left unforced until something demands it, matching GHC and
-- SPEC's eagerness contract ("bottom is never evaluated eagerly"). Before the
-- fix, binding or sequencing a `return`ed value through the IO path forced it to
-- WHNF immediately, so `_ <- return (error "x")` raised where GHC does not.
--
-- The bottom below is a genuine thunked application (`error ...`), not a folded
-- constant. Reaching each putStrLn proves the bottom was NOT forced; the final
-- `try` proves it STILL raises when actually demanded (laziness, not
-- error-swallowing).

boom :: Integer
boom = error "boom: return forced its argument"

-- Terminal `return bottom` in a function body (the do-block terminal path).
returnsBottom :: IO Integer
returnsBottom = return boom

main :: IO ()
main = do
    -- 1. Discarded bind of a returned bottom must NOT raise.
    _ <- return boom
    putStrLn "1: `_ <- return boom` did not force"

    -- 2. A bound returned bottom is inert until demanded (we never touch v here).
    v <- return boom
    putStrLn "2: `v <- return boom` bound without forcing"

    -- 3. `pure $ bottom` — the ($)-applied form — is lazy too.
    _ <- pure $ boom
    putStrLn "3: `pure $ boom` did not force"

    -- 4. A function whose terminal action is `return bottom`, then discarded.
    _ <- returnsBottom
    putStrLn "4: `_ <- returnsBottom` (terminal return of bottom) did not force"

    -- 5. fmap with a non-strict function over a returned bottom keeps it lazy:
    --    fmap const (return ⊥) ~ return (const 99 ⊥) = return 99.
    w <- fmap (\_ -> 99 :: Integer) (return boom)
    assert (w == 99) "5: fmap const over (return bottom) does not force the bottom"

    -- 6. Structured laziness is preserved: a returned tuple's bottom field is
    --    not forced (fst/snd laziness holds through return).
    p <- return (7 :: Integer, boom)
    assert (fst p == 7) "6: returned tuple keeps its bottom field lazy"

    -- 7. The Maybe monad's return stays lazy as well (regression guard).
    case (return boom :: Maybe Integer) of
        Just _  -> putStrLn "7: Maybe return does not force its argument"
        Nothing -> error "impossible"

    -- 8. ...but a DEMANDED returned bottom STILL raises. `seq` forces `v` to
    --    WHNF inside the tried action, so the error is raised and caught there.
    --    Non-strictness is not error-swallowing.
    r <- try (v `seq` pure ())
    case r of
        Right () -> error "8: forcing a returned bottom must raise"
        Left _   -> putStrLn "8: demanding the returned bottom raises when forced"

    putStrLn "return/pure are non-strict: bottom is not forced until demanded"
