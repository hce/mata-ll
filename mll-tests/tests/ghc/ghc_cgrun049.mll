-- GHC cgrun049: Maybe monad chain
-- Tests Maybe >>= for short-circuiting computation

safeDivM :: Integer -> Integer -> Maybe Integer
safeDivM _ 0 = Nothing
safeDivM a b = Just (a `div` b)

-- Chain: 100 / a / b / c
chain :: Integer -> Integer -> Integer -> Maybe Integer
chain a b c = safeDivM 100 a >>= \r1 -> safeDivM r1 b >>= \r2 -> safeDivM r2 c

lookupM :: String -> [(String, Integer)] -> Maybe Integer
lookupM _ [] = Nothing
lookupM k ((k2, v):rest)
    | k == k2   = Just v
    | otherwise  = lookupM k rest

main :: IO ()
main = do
    assert (chain 2 5 2 == Just 5) "chain ok"
    assert (chain 0 5 2 == Nothing) "chain fail first"
    assert (chain 2 0 2 == Nothing) "chain fail second"
    assert (chain 2 5 0 == Nothing) "chain fail third"

    -- Maybe as error handling
    let table = [("width", 10), ("height", 20)]
    let area = lookupM "width" table >>= \w ->
               lookupM "height" table >>= \h ->
               Just (w * h)
    assert (area == Just 200) "maybe lookup chain"

    let bad = lookupM "width" table >>= \w ->
              lookupM "depth" table >>= \d ->
              Just (w * d)
    assert (bad == Nothing) "maybe lookup fail"

    -- fmap on result of >>=
    assert (fmap (* 2) (Just 5 >>= \x -> Just (x + 1)) == Just 12) "fmap after bind"

    putStrLn "ok"
