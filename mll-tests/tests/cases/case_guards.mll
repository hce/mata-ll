-- Guards on case branches (SPEC: "Guards are supported on function
-- definitions and case branches"). A branch whose pattern matches but
-- whose guards all fail must fall through to the next branch.

classify :: Integer -> String
classify n = case n of
    0             -> "zero"
    n | n > 0     -> "positive"
      | otherwise -> "negative"

-- Guard fallthrough across a constructor-pattern boundary: `Just 7`
-- matches `Just n` but the guard fails, so it must reach `_`.
describe :: Maybe Integer -> String
describe m = case m of
    Just 0           -> "exact zero"
    Just n | n > 100 -> "big"
           | n < 0   -> "neg"
    _                -> "other"

main :: IO ()
main = do
    assert (classify 0 == "zero") "classify zero"
    assert (classify 7 == "positive") "classify positive"
    assert (classify (-4) == "negative") "classify negative"
    assert (describe (Just 0) == "exact zero") "describe exact zero"
    assert (describe (Just 250) == "big") "describe big"
    assert (describe (Just (-3)) == "neg") "describe neg"
    assert (describe (Just 7) == "other") "describe fallthrough to wildcard"
    assert (describe Nothing == "other") "describe Nothing"
    putStrLn "case guards: OK"
