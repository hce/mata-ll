-- GHC cgrun041: Laziness and non-strict semantics
-- Tests that unevaluated expressions are not forced prematurely

-- const should not force its second argument
constTest :: Int
constTest = const 42 undefined

-- head should not force the tail
headTest :: Int
headTest = head (1 : undefined)

-- if-then-else short circuits
ifTest :: Int
ifTest = if True then 42 else undefined

-- infinite list via top-level recursion
ones :: [Int]
ones = 1 : ones

main :: IO ()
main = do
    assert (constTest == 42) "const skips undefined"
    assert (headTest == 1) "head skips tail"
    assert (ifTest == 42) "if short circuits"

    -- Lazy list construction: infinite list, only take a finite prefix
    assert (take 5 ones == [1, 1, 1, 1, 1]) "take from infinite"
    assert (head ones == 1) "head infinite"

    putStrLn "ok"
