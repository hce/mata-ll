-- Test: a LOCAL binding that shadows a polymorphic top-level function is
-- the local everywhere it is used. The monomorphizer tracked lambda and
-- let-expression binders as locals but not do-block binders (`x <- m`,
-- do-`let`), where-bound names, or a where-bound function's own
-- parameters, so a use of such a local at a monomorphic type was rewritten
-- into a call to a freshly minted specialization of the GLOBAL
-- (`reverse` below became `reverse_String`).

data Wrap = Wrap [Int]

useWhere :: [Int] -> [Int]
useWhere xs = reverse xs
  where
    reverse ys = 0 : ys

whereParam :: [Int] -> [Int]
whereParam xs = go xs
  where
    go replicate = replicate

lambdaShadow :: [Int] -> [Int]
lambdaShadow = \concat -> concat

main :: IO ()
main = do
    -- do-bind shadowing `reverse :: [a] -> [a]`
    reverse <- return "abc"
    assert (reverse == "abc") "do-bound name shadows the global"
    -- do-let shadowing `replicate :: Int -> a -> [a]`
    let replicate = 7 :: Int
    assert (replicate + 1 == 8) "do-let name shadows the global"
    -- do-let FUNCTION shadowing `concat`
    let concat n = n * 2 :: Int
    assert (concat 21 == 42) "do-let function shadows the global"
    -- pattern-bound shadow in a do-block
    (length, _) <- return (5 :: Int, 6 :: Int)
    assert (length == 5) "do pattern-bound name shadows the global"
    -- where-bound shadow, and a where-function parameter shadow
    assert (useWhere [1, 2] == [0, 1, 2]) "where-bound function shadows the global"
    assert (whereParam [3] == [3]) "where-function parameter shadows the global"
    assert (lambdaShadow [4] == [4]) "lambda parameter shadows the global"
