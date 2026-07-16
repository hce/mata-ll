-- Closed type-family clause priority (audit finding 12, doc/audit/t7):
-- the FIRST source-order clause whose pattern matches wins. Before the fix
-- the eager AST-level matcher had no promoted-constructor case, so `F 'Z`
-- failed the specific clause and fell through to the catch-all — the
-- catch-all beat every earlier specific clause.
data Nat = Z | S Nat

type family F n where
    F 'Z = Integer
    F n  = String

-- The specific clause wins for 'Z...
valZ :: F 'Z
valZ = 5

-- ...and a ground argument that genuinely falls through clause 1 (it is
-- APART from 'Z) reaches the catch-all.
valS :: F ('S 'Z)
valS = "one"

-- Non-linear patterns select by consistency: Same a a only matches when
-- both arguments are equal, and a ground mismatch is apart from it.
type family Same a b where
    Same a a = 'True
    Same a b = 'False

data BoolW b where
    WT :: BoolW 'True
    WF :: BoolW 'False

eqII :: BoolW (Same Integer Integer)
eqII = WT

neIB :: BoolW (Same Integer Bool)
neIB = WF

tag :: BoolW b -> String
tag WT = "same"
tag WF = "different"

main :: IO ()
main = do
    print valZ
    putStrLn valS
    putStrLn (tag eqII)
    putStrLn (tag neIB)
    if valZ == 5 && valS == "one"
        then putStrLn "ok clause-priority"
        else error "FAIL clause-priority"
