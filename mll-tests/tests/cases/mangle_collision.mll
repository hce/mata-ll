-- Regression: two DISTINCT types whose readable name-mangling collides.
-- `(A_B, C)` and `(A, B_C)` both suffix to "TupA_B_C", so the generated
-- tuple show/eq implementations used to share one Lua function name — the
-- second definition clobbered the first, printing the wrong constructor
-- names and calling element functions of the wrong type. Specialization is
-- keyed on the structured type (never on strings), and genuine name
-- collisions get a deterministic "__2" disambiguator.

data A_B = MkAB deriving (Show, Eq)
data A   = MkA  deriving (Show, Eq)
data B_C = MkBC deriving (Show, Eq)
data C   = MkC  deriving (Show, Eq)

main :: IO ()
main = do
    assert (show (MkAB, MkC) == "(MkAB,MkC)") "show (A_B, C)"
    assert (show (MkA, MkBC) == "(MkA,MkBC)") "show (A, B_C)"
    assert ((MkAB, MkC) == (MkAB, MkC)) "eq (A_B, C)"
    assert ((MkA, MkBC) == (MkA, MkBC)) "eq (A, B_C)"
    putStrLn "ok"
