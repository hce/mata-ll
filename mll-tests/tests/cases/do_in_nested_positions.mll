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

main :: IO ()
main = do
    greet Cat
    report 3
    report 0
    snd pairAct
    putStrLn (show (fst pairAct))
-- expect: hello
-- expect: cat
-- expect: positive
-- expect: 3
-- expect: non-positive
-- expect: in tuple
-- expect: second
-- expect: 2
