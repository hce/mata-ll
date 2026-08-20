-- Guarded multi-clause functions emit each clause as an independent Lua
-- block. Regression: only the where-scope rows were restored between
-- clauses, so a pattern local bound in clause 1 stayed registered as a
-- local — a later clause's reference to a SAME-NAMED top-level function
-- then resolved to the bare (nil) local name instead of the function's
-- slot ("attempt to call a nil value"), and a same-named where-binding in
-- a later clause skipped its forward declaration and assigned a global.

-- top-level function whose name clause 1 binds as a pattern local
h :: Int -> Int
h n = n * 10

-- clause 1 binds `h` from the pattern (with a guard, so the guarded
-- emitter is used); clause 2 calls the TOP-LEVEL h
test :: Maybe Int -> Int -> Int
test (Just h) k | h > 0 = h + k
test _ k | otherwise = h k

-- same-name hazard through a later clause's where-binding
test2 :: Maybe Int -> Int
test2 (Just w) | w > 100 = w
test2 m | otherwise = w + 1
  where w = 7

main :: IO ()
main = do
    print (test (Just 5) 1)     -- clause 1: 5 + 1 = 6
    print (test (Just 0) 3)     -- guard fails, falls to clause 2: h 3 = 30
    print (test Nothing 4)      -- clause 2: h 4 = 40
    print (test2 (Just 200))    -- clause 1: 200
    print (test2 (Just 1))      -- guard fails, clause 2: 7 + 1 = 8
    print (test2 Nothing)       -- clause 2: 8
