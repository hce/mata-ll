-- The HashMap flavor of the exact-first-force eagerization pins (see
-- exact_let_eager.mll for the GHC-goldened general shapes — hm builtins
-- are outside the GHC oracle, so this file self-asserts; the expected
-- values were computed with a Data.Map twin under runghc). The churn
-- shape is the one the optimization was built for: both derived map
-- versions sit on the scrutinee's first-force chain (hmLookup forces m2
-- at entry with a bottom-free key, hmDelete forces m1 the same way) and
-- are emitted as direct eager calls, no thunks. The laziness pins keep
-- it honest: a binding off the chain must never run its `error`.
module Main where

churn :: Int -> Int -> HashMap Int Int -> Int
churn 0 acc m = acc + hmSize m
churn i acc m =
    let m1 = hmInsert (i `mod` 7) (i * i) m
        m2 = hmDelete ((i * 3) `mod` 7) m1
    in case hmLookup ((i * 2) `mod` 7) m2 of
        Just v  -> churn (i - 1) (acc + v) m2
        Nothing -> churn (i - 1) acc m2

-- An alias is a chain link: forcing `b` forces `a` with nothing in
-- between, so both eagerize.
aliased :: HashMap Int Int -> Int
aliased m =
    let a = hmInsert 1 10 m
        b = a
    in case hmSize b of
        0 -> -1
        n -> n * 100

-- A binding demanded in only ONE branch is not on the chain: the taken
-- branch never forces `dead`.
branchLazy :: HashMap Int Int -> Int
branchLazy m =
    let dead = hmSize (error "branchLazy: must never run" :: HashMap Int Int)
    in case hmLookup 1 m of
        Just v  -> v
        Nothing -> dead

-- An if-condition anchors exactly like a case scrutinee.
condAnchor :: HashMap Int Int -> Int
condAnchor m =
    let m' = hmInsert 5 50 m
    in if hmMember 5 m' then hmSize m' else 0

-- The where flavor of the hm chain.
whereChainHm :: HashMap Int Int -> Int
whereChainHm m = case hmLookup 2 m2 of
    Just v  -> v + hmSize m2
    Nothing -> hmSize m2
  where
    m1 = hmInsert 2 20 m
    m2 = hmInsert 3 30 m1

main :: IO ()
main = do
    assert (churn 500 0 hmEmpty == 17839325) "churn (Data.Map twin value)"
    assert (aliased hmEmpty == 100) "aliased"
    assert (branchLazy (hmInsert 1 11 hmEmpty) == 11) "branchLazy stays lazy"
    assert (condAnchor hmEmpty == 1) "condAnchor"
    assert (whereChainHm hmEmpty == 22) "whereChainHm (Data.Map twin value)"
    putStrLn "exact_let_eager_hm ok"
