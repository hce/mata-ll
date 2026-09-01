-- The fused `case hmLookup k m of …` emission (try_fused_hm_lookup_case):
-- a raw slot read plus nil test, no Just cell. Pins the shapes the fusion
-- must keep byte-identical to the general path: a hit, a miss, a stored
-- NIL value riding the __mll_hm_nilv sentinel (Just Nothing must come
-- back, not Nothing), both branch orders, the wildcard Just pattern, and
-- a guarded case (NOT fused — the general path must still run).
module Main where

m1 :: HashMap Int Int
m1 = hmInsert 1 11 (hmInsert 2 22 hmEmpty)

m2 :: HashMap Int (Maybe Int)
m2 = hmInsert 7 Nothing (hmInsert 8 (Just 88) hmEmpty)

hit :: Int -> Int
hit k = case hmLookup k m1 of
    Just v  -> v
    Nothing -> -1

hitFlip :: Int -> Int
hitFlip k = case hmLookup k m1 of
    Nothing -> -1
    Just v  -> v

hitWild :: Int -> String
hitWild k = case hmLookup k m1 of
    Just _  -> "present"
    Nothing -> "absent"

nilValue :: Int -> String
nilValue k = case hmLookup k m2 of
    Just Nothing  -> "stored-nothing"
    Just (Just v) -> "stored-just " <> show v
    Nothing       -> "absent"

-- The FUSED path over a stored nil: Just v with v bound to the
-- sentinel-unwrapped Nothing — the unwrap happens in the emitted branch.
justBind :: Int -> String
justBind k = case hmLookup k m2 of
    Just v  -> "got " <> show v
    Nothing -> "absent"

guarded :: Int -> String
guarded k = case hmLookup k m1 of
    Just v | v > 15    -> "big"
           | otherwise -> "small"
    Nothing            -> "none"

main :: IO ()
main = do
    print (hit 1)
    print (hit 2)
    print (hit 3)
    print (hitFlip 1)
    print (hitFlip 9)
    putStrLn (hitWild 2)
    putStrLn (hitWild 5)
    putStrLn (nilValue 7)
    putStrLn (nilValue 8)
    putStrLn (nilValue 9)
    putStrLn (justBind 7)
    putStrLn (justBind 8)
    putStrLn (justBind 9)
    putStrLn (guarded 1)
    putStrLn (guarded 2)
    putStrLn (guarded 4)

-- expect: 11
-- expect: 22
-- expect: -1
-- expect: 11
-- expect: -1
-- expect: present
-- expect: absent
-- expect: stored-nothing
-- expect: stored-just 88
-- expect: absent
-- expect: got Nothing
-- expect: got Just 88
-- expect: absent
-- expect: small
-- expect: big
-- expect: none
