-- Test: `do` blocks in the expression positions the desugarer once walked
-- by hand and skipped — a case-branch GUARD body, a TUPLE element, and a
-- CLASS DEFAULT method. Each reached the typechecker undesugared and hit
-- its "Do should be desugared" unreachable arm (a compiler panic).

class Greet a where
    name :: a -> String
    greet :: a -> IO ()
    greet x = do
        putStrLn "hello"
        putStrLn (name x)

data Cat = Cat

instance Greet Cat where
    name _ = "cat"

report :: Int -> IO ()
report n = case n of
    m | m > 0 -> do
            putStrLn "positive"
            putStrLn (show m)
      | otherwise -> do
            putStrLn "non-positive"

pairAct :: (Int, IO ())
pairAct = (2, do putStrLn "in tuple"
                 putStrLn "second")

-- The same three positions, returning values the checks below assert on.
classify :: Int -> IO String
classify n = case n of
    m | m > 0 -> do
            let s = "positive " <> show m
            return s
      | otherwise -> do
            return "non-positive"

class Named a where
    nameOf :: a -> String
    greeting :: a -> IO String
    greeting x = do
        let g = "hello " <> nameOf x
        return g

instance Named Cat where
    nameOf _ = "cat"

pairVal :: (Int, IO Int)
pairVal = (2, do let x = 20
                 return (x + 1))

main :: IO ()
main = do
    greet Cat
    report 3
    report 0
    snd pairAct
    putStrLn (show (fst pairAct))
    p <- classify 3
    assert (p == "positive 3") "do in a case-guard body"
    q <- classify 0
    assert (q == "non-positive") "do in the otherwise guard body"
    g <- greeting Cat
    assert (g == "hello cat") "do in a class default method"
    v <- snd pairVal
    assert (v == 21 && fst pairVal == 2) "do in a tuple element"
