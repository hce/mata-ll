-- The fold pass splices saturated all-literal-argument calls to small
-- monomorphic wrapper functions (`print 23` → `putStrLn (show_Int 23)`
-- → the FFI call itself) and folds `show` of Int/Bool literals to the
-- shown string, so a constant program finishes its work at compile
-- time.  These are exact beta-reductions: everything observable —
-- output bytes, bottom placement, re-performability — must match GHC.

-- the print → putStrLn (show x) chain collapses at a literal call site
constOut :: Int
constOut = 40000 + 1201

-- a spliced body that can raise must still raise only when demanded:
-- `kept` becomes `7 div 0` inline (the trap is declined by the literal
-- folds) and the True branch never demands it
overZero :: Int -> Int
overZero x = x `div` 0

kept :: Int
kept = overZero 7

pick :: Bool -> Int
pick b = if b then 31 else kept

-- an action value built from a spliced FFI wrapper stays re-performable
act :: IO ()
act = putStrLn "act"

-- a wrapper of a wrapper: the acyclic candidate chain splices through
shout :: String -> IO ()
shout s = putStrLn s

louder :: String -> IO ()
louder s = shout s

main :: IO ()
main = do
    print constOut
    print (pick True)
    print True
    print False
    print (-23)
    act
    act
    louder "twice-wrapped"
