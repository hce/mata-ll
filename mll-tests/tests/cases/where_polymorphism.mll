-- A19: where-bindings GENERALIZE like let-bindings — the bindings are
-- inferred before the clause body (they used to be typed after it, so the
-- body's uses drove them through one shared monomorphic variable), and
-- each unconstrained variable generalizes over the clause environment. A
-- helper used at two types works; the widened-instantiation arity hole
-- this opened for locals is closed by the local_fn_arity split machinery
-- (a generalized `helper k = k` used at a function type is called through
-- the same over-application split as a top-level callee). What stays
-- monomorphic, deliberately (Q37, documented in HASKDIFF): variables
-- carrying an unresolved CLASS constraint — one Lua closure cannot be
-- class-polymorphic.

pair :: ([Int], [String])
pair = (go 1, go "s")
  where go y = [y]

-- the arity-widening shape: a generalized identity-ish local used at a
-- function type, saturated through the clause's eta padding
viaHelper :: (Int -> Int -> String) -> Int -> Int -> String
viaHelper h = helper h
  where helper k = k

-- generalization coexisting with pattern params (A14) and recursion
lengths :: ([Int], Int)
lengths = (sizes [(1, "a"), (2, "b")], count [True, False, True])
  where
    sizes ps = map fst ps
    count [] = 0
    count (_ : xs) = 1 + count xs

main :: IO ()
main = do
    print pair
    putStrLn (viaHelper (\a b -> show (a + b)) 20 1)
    print lengths
