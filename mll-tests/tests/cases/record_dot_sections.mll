-- `(.field)` and `(.a.b)` are record field selector sections
-- (OverloadedRecordDot): `\r -> r.field`. They parsed as right sections of
-- the composition operator ("Cannot unify 'Int -> a' with 'P'"). The
-- whitespace rule is GHC's: `(. f)` with a space is still composition.

data P = P { px :: Int, py :: Int } deriving Show
data W = W { inner :: P, tag :: String } deriving Show

mk :: Int -> P
mk n = P n (n * 2)

main :: IO ()
main = do
    print (map (.px) [mk 1, mk 2])
    print (map (.py) [mk 1, mk 2])
    print (map (.inner.py) [W (mk 3) "t", W (mk 4) "u"])
    print (map (.tag) [W (mk 3) "t"])
    print (sum (map (.px) [mk 1, mk 2, mk 3]))
    print (map (negate . abs) [1, -2 :: Int], (. (+ 1)) (* 2) (5 :: Int))
    print (filter ((> 2) . (.px)) [mk 1, mk 3])
