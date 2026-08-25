-- GADT exhaustiveness at a RIGID scrutinee index: `f :: G b -> …` can
-- receive every constructor (the caller chooses b), so all of them are
-- required.  Regression: the coverage filter excluded a constructor
-- whenever its result type failed to UNIFY with the scrutinee type —
-- and the first clause's GADT refinement had already pinned the
-- signature index (`G b` checked as `G Int`), so the other indices'
-- constructors were silently dropped and a partial match compiled
-- (then crashed at runtime on `f MkBool`).  The accept side: at a
-- CONCRETE index the apart constructors stay excluded, so a
-- single-constructor match remains exhaustive.

data G a where
    MkInt  :: G Int
    MkBool :: G Bool

-- concrete index: MkBool (:: G Bool) is apart from G Int — exhaustive
onlyInt :: G Int -> Int
onlyInt MkInt = 2

-- rigid index: both constructors required (and both handled here)
handle :: G b -> Int
handle MkInt = 1
handle MkBool = 0

main :: IO ()
main = do
    print (onlyInt MkInt)
    print (handle MkInt)
    print (handle MkBool)
