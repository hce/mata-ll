-- Negative literal patterns in ATOM position must be parenthesized
-- (Haskell 2010: apat has no negative literal).  Regression: `-N` was
-- treated as a pattern-atom start, so `f (Just -1)` parsed where GHC
-- parse-errors (rejection pinned in compile_errors.rs).  The accept
-- side: the parenthesized argument form and the whole-pattern case
-- branch form.

f :: Maybe Int -> Int
f (Just (-1)) = 0
f _ = 1

g :: Int -> String
g n = case n of
    -1 -> "neg one"
    _ -> "other"

main :: IO ()
main = do
    print (f (Just (-1)))
    print (f (Just 5))
    putStrLn (g (-1))
    putStrLn (g 3)
