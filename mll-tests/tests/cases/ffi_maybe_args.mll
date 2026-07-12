-- Regression: SPEC "Optional parameters" — a `Maybe` argument in an FFI
-- signature is an optional Lua parameter. `Just x` passes the unwrapped `x`
-- (never the raw Just table); a `Nothing` in the trailing run of Maybe
-- parameters is genuinely OMITTED from the call, not passed as nil
-- (math.min/math.max/math.random are argument-count-sensitive and raise
-- "bad argument #N (number expected, got nil)" on an explicit nil).
-- A Maybe parameter that sits before another passed argument cannot be
-- positionally omitted in Lua; it is passed as an explicit nil — Lua's own
-- idiom for a skipped middle optional (string.find treats a nil init as 1).

-- The exact SPEC signature: math.random with an optional upper bound
rnd :: Number -> Maybe Number -> LuaIO "math.random" Number

-- Trailing single Maybe, pure path (math.min(5, nil) raises; math.min(5) is fine)
mn :: Number -> Maybe Number -> LuaPure "math.min" Number

-- Two trailing Maybes (math.max also arg-count-sensitive)
mx :: Number -> Maybe Number -> Maybe Number -> LuaPure "math.max" Number

-- IO path
mnIO :: Number -> Maybe Number -> LuaIO "math.min" Number

-- Catch path: middle Nothing before a passed Just -> explicit nil -> host rejects -> Left
mxC :: Number -> Maybe Number -> Maybe Number -> LuaCatch "math.max" (Either String Number)

-- Non-trailing Maybe followed by a required param (string.find treats nil init
-- as default 1); Maybe FFI *result* decoding must be undisturbed
find' :: String -> String -> Maybe Number -> Bool -> LuaPure "string.find" (Maybe Number)

-- Method-call FFI with a trailing Maybe (string:rep(n, sep))
rep' :: String -> Number -> Maybe String -> LuaPure ":rep" String

isLeft' :: Either String Number -> Bool
isLeft' (Left _) = True
isLeft' (Right _) = False

main :: IO ()
main = do
    r1 <- rnd 3.0 Nothing
    assert (r1 >= 1.0 && r1 <= 3.0) "random omit (math.random(3))"
    r2 <- rnd 1.0 (Just 6.0)
    assert (r2 >= 1.0 && r2 <= 6.0) "random just (math.random(1, 6))"
    assert (mn 5.0 Nothing == 5.0) "min omit"
    assert (mn 5.0 (Just 2.0) == 2.0) "min just"
    assert (mx 1.0 Nothing Nothing == 1.0) "max omit both"
    assert (mx 1.0 (Just 5.0) Nothing == 5.0) "max just/omit"
    assert (mx 1.0 (Just 5.0) (Just 9.0) == 9.0) "max just/just"
    a <- mnIO 5.0 Nothing
    b <- mnIO 5.0 (Just 2.0)
    assert (a == 5.0) "minIO omit"
    assert (b == 2.0) "minIO just"
    assert (isLeft' (mxC 1.0 Nothing (Just 7.0))) "middle nil in trailing run -> host error -> Left"
    assert (find' "abcabc" "b" Nothing True == Just 2.0) "find nil init"
    assert (find' "abcabc" "b" (Just 3.0) True == Just 5.0) "find just init"
    assert (find' "hello" "z" Nothing True == Nothing) "find no match (Maybe result)"
    assert (rep' "ab" 2.0 Nothing == "abab") "rep omit sep"
    assert (rep' "ab" 2.0 (Just "-") == "ab-ab") "rep just sep"
    putStrLn "all ok"
