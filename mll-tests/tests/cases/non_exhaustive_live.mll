-- Dead-branch cleanup (codegen/opt.rs pass 2 and the exhaustive-match
-- else emission in pattern.rs) must not remove a LIVE fall-off: a guard
-- chain whose last guard is not `otherwise` still raises Non-exhaustive
-- when no guard holds. (A partial constructor match is rejected by the
-- typechecker, so guards are the only source of runtime fall-offs.)
-- The exhaustive cases around it check the else-converted last clause
-- still binds and evaluates correctly.

sign :: Integer -> String
sign n | n < 0 = "neg"
       | n > 0 = "pos"

data Three = A | B | C deriving (Show, Eq)

total3 :: Three -> String
total3 A = "a"
total3 B = "b"
total3 C = "c"

fromJust' :: Maybe Integer -> Integer
fromJust' (Just x) = x + 1
fromJust' Nothing = 0

main :: IO ()
main = do
    assert (sign 3 == "pos") "guard arm still selected"
    assert (total3 C == "c") "exhaustive last clause reached as else"
    assert (fromJust' (Just 41) == 42) "else-converted clause binds its fields"
    assert (fromJust' Nothing == 0) "two-arm complement keeps both arms"
    -- putStr forces its argument inside the action, so the non-exhaustive
    -- error is raised inside try (a `pure` would defer it past the pcall).
    r1 <- try (putStr (sign 0))
    case r1 of
        Left _  -> putStr "."
        Right _ -> error "sign 0 must be non-exhaustive"
