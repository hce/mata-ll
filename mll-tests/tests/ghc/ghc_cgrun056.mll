-- GHC cgrun056: Balanced parentheses checker

data Paren = Open | Close
    deriving (Show, Eq)

balanced :: [Paren] -> Bool
balanced parens = go parens 0
  where
    go [] depth      = depth == 0
    go (p:ps) depth
        | depth < 0  = False
        | p == Open  = go ps (depth + 1)
        | otherwise  = go ps (depth - 1)

rep :: a -> Integer -> [a]
rep _ 0 = []
rep x n = x : rep x (n - 1)

main :: IO ()
main = do
    assert (balanced []) "empty is balanced"
    assert (balanced [Open, Close]) "() balanced"
    assert (balanced [Open, Open, Close, Close]) "(()) balanced"
    assert (balanced [Open, Close, Open, Close]) "()() balanced"
    assert (not (balanced [Open])) "( not balanced"
    assert (not (balanced [Close])) ") not balanced"
    assert (not (balanced [Close, Open])) ")( not balanced"
    assert (not (balanced [Open, Open, Close])) "(() not balanced"
    assert (not (balanced [Open, Close, Close, Open, Close])) ")( variant"

    -- Deep nesting
    let deep = appendList (rep Open 10) (rep Close 10)
    assert (balanced deep) "10 deep balanced"
    putStrLn "ok"

appendList :: [a] -> [a] -> [a]
appendList [] ys = ys
appendList (x:xs) ys = x : appendList xs ys
