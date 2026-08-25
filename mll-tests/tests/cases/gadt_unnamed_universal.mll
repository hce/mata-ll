-- A GADT signature may bind universal variables under names the data
-- header doesn't use: `MkAny :: b -> Box b` under `data Box a where`.
-- They are universals exactly like the header's, so every use site must
-- instantiate them fresh.  Regression: the constructor scheme (and the
-- pattern-side instantiation list) quantified only header-named and
-- existential variables, so every use of MkAny shared ONE literal `b` —
-- two uses at different payload types in one clause failed with a
-- spurious "Cannot unify 'Int' with 'String'" (confirmed by repro).

data Box a where
    MkAny :: b -> Box b
    MkTwo :: c -> c -> Box c

unbox :: Box a -> a
unbox (MkAny v) = v
unbox (MkTwo x _) = x

both :: Box a -> Box a -> (a, a)
both (MkAny x) (MkAny y) = (x, y)
both b1 b2 = (unbox b1, unbox b2)

main :: IO ()
main = do
    -- two expression uses at different types in one clause
    print (unbox (MkAny (1 :: Int)) + 1)
    putStrLn (unbox (MkAny "two"))
    -- two PATTERN uses in one clause, again at per-use types
    print (both (MkAny (3 :: Int)) (MkAny 4))
    case both (MkAny "a") (MkAny "b") of
        (x, y) -> putStrLn (x <> y)
    print (unbox (MkTwo (5 :: Int) 6))
