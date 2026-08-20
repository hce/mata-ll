-- max/min are Ord class methods (GHC parity), dispatched per type like
-- `compare` — NOT unconstrained builtins lowered to Lua math.max/math.min.
-- Regression: the builtin lowering typechecked `max` at EVERY type and
-- crashed at runtime on anything that is not a Lua number — String, Bool,
-- and (because unannotated literals default to Integer, which is boxed)
-- even `print (max 3 5)`.
--
-- GHC's default bodies fix the tie sides:
--   max x y = if x <= y then y else x   -- tie returns the SECOND argument
--   min x y = if x <= y then x else y   -- tie returns the FIRST argument
-- Observable through the one Eq-equal-but-distinguishable pair: 0.0 == -0.0.

data Sev = Low | Mid | High deriving (Show, Eq, Ord)
data Q = Q Int String deriving (Show, Eq, Ord)
newtype Age = Age Int deriving (Show, Eq, Ord)

-- Polymorphic Ord-constrained body, used at several types: max/min must
-- dispatch through the constraint, not a hardcoded lowering.
biggest :: Ord a => a -> a -> a -> a
biggest a b c = max a (max b c)

main :: IO ()
main = do
    -- Defaulted Integer literals (the headline crash) and big boxed values
    print (max 3 5)
    print (min 3 5)
    print (max 1180591620717411303424 1152921504606846976)
    print (min (-1180591620717411303424) 7)

    -- Annotated Int / Number
    print (max (3 :: Int) 5)
    print (min (-3 :: Int) (-5))
    print (max (2.5 :: Number) (-7.25))
    print (min (2.5 :: Number) (-7.25))

    -- Ties: 0.0 == -0.0 but they show differently, so the tie side is
    -- observable. max returns its second argument, min its first.
    print (max (0.0 :: Number) (-0.0))
    print (max (-0.0 :: Number) 0.0)
    print (min (0.0 :: Number) (-0.0))
    print (min (-0.0 :: Number) 0.0)

    -- String and Unit
    putStrLn (max "abc" "abd")
    putStrLn (min "abc" "abd")
    putStrLn (max "b" "abc")
    print (max () ())

    -- Derived Ord: enum, fielded (lexicographic), newtype
    print (max Low High)
    print (min Mid Low)
    print (max (Q 1 "b") (Q 1 "a"))
    print (min (Q 2 "a") (Q 1 "z"))
    print (max (Age 3) (Age 7))

    -- Dispatch inside a polymorphic Ord-constrained body
    print (biggest (2 :: Int) 9 4)
    putStrLn (biggest "m" "z" "a")
    print (biggest Low High Mid)

    -- Interaction with the rest of the Ord machinery
    assert (max 3 5 == 5) "max eq consistency"
    assert (compare (max Low Mid) Mid == EQ) "max compare consistency"
    putStrLn "all max/min tests passed"
