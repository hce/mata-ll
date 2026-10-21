-- A binder's concreteness claim is a property of that binding alone: an
-- inner binder named like a concrete enclosing name — a strict parameter
-- (p), a top-level function (tot), a record field selector (fld), a
-- where-bound function (go), a strict let-bound local (s), a do-bound
-- variable (d) — holds whatever ITS source delivers (here always a
-- suspended list element, tuple field or action result) and must be read
-- forced. Every binding form that introduces a Lua local is exercised
-- against the enclosing kinds in its scope. 2026-10 fresh-eyes review (L5).
module Main where

tot :: Int
tot = 1000

data R = R { fld :: Int, tag :: String }

lazies :: Int -> [Int]
lazies n = map (\k -> k * 2) [n, n + 1, n + 2, n + 3, n + 4, n + 5]

lazyPair :: Int -> (Int, Int)
lazyPair n = (head (lazies n), lazies n !! 1)

lazySix :: Int -> (Int, Int, Int, Int, Int, Int)
lazySix n = case lazies n of
    (a : b : c : e : f : g : _) -> (a, b, c, e, f, g)
    _ -> (0, 0, 0, 0, 0, 0)

safeHead :: [Int] -> Maybe Int
safeHead (x : _) = Just x
safeHead [] = Nothing

caseValue :: Int -> IO ()
caseValue p = do
    d <- pure (p + 5)
    let s = p * 2
    print (go s + d + p)
    print (case lazies p of
        (p : tot : fld : go : s : d : _) -> p + tot + fld + go + s + d
        _ -> -1)
  where
    go q = q + 1

caseAction :: Int -> IO ()
caseAction p = do
    d <- pure (p + 5)
    let s = p * 2
    print (go s + d + p)
    case lazies p of
        (p : tot : fld : go : s : d : _) -> print (p + tot + fld + go + s + d)
        _ -> print (-1)
  where
    go q = q + 1

caseGuarded :: Int -> IO ()
caseGuarded p = do
    d <- pure (p + 5)
    let s = p * 2
    print (go s + d + p)
    print (case lazies p of
        (p : tot : fld : go : s : d : _)
            | p > 100 -> 0
            | p == 2 -> p + tot + fld + go + s + d
        _ -> -1)
    case lazies p of
        (p : tot : fld : go : s : d : _)
            | p == 7 -> print 0
            | otherwise -> print (p * tot * fld + go + s + d)
        _ -> print (-1)
  where
    go q = q + 1

localClauses :: Int -> IO ()
localClauses p = do
    d <- pure (p + 5)
    let s = p * 2
    print (go s + d + p)
    let pick (p : tot : fld : go : s : d : _) = p + tot + fld + go + s + d
        pick _ = -1
    print (pick (lazies p))
    print (sel (lazies p))
  where
    go q = q + 1
    s = p * 3
    sel (p : tot : fld : go : s : _) = p + tot + fld + go + s
    sel _ = -1

lambdas :: Int -> IO ()
lambdas p = do
    d <- pure (p + 5)
    let s = p * 2
    print (go s + d + p)
    let six = lazySix p
    let lz = lazies p
    print ((\p tot fld go s d -> p + tot + fld + go + s + d) (lz !! 0) (lz !! 1) (lz !! 2) (lz !! 3) (lz !! 4) (lz !! 5))
    print ((\(p, tot, fld, go, s, d) -> p + tot + fld + go + s + d) six)
    print ((\(Just p) -> p * 3) (Just (head lz)))
  where
    go q = q + 1

letForms :: Int -> IO ()
letForms p = do
    d <- pure (p + 5)
    let s = p * 2
    print (go s + d + p)
    let six = lazySix p
    let lz = lazies p
    let (p, tot, fld, go, s, d) = six
    print (p + tot + fld + go + s + d)
    let p = head lz
        tot = lz !! 1
        fld = lz !! 2
        go = lz !! 3
        s = lz !! 4
        d = lz !! 5
    print (p + tot + fld + go + s + d)
    print (let (p, tot) = lazyPair 3 in p * tot)
  where
    go q = q + 1

whereValues :: Int -> [Int] -> Int
whereValues p lz = go s + inner
  where
    go q = q + 1
    s = p * 2
    inner = p + tot + fld + go + s
      where
        p = head lz
        tot = lz !! 1
        fld = lz !! 2
        go = lz !! 3
        s = lz !! 4

doBinds :: Int -> IO ()
doBinds p = do
    d <- pure (p + 5)
    let s = p * 2
    print (go s + d + p)
    let lz = lazies p
    p <- pure (head lz)
    tot <- pure (lz !! 1)
    fld <- pure (lz !! 2)
    go <- pure (lz !! 3)
    s <- pure (lz !! 4)
    d <- pure (lz !! 5)
    print (p + tot + fld + go + s + d)
    (p : tot : fld : go : s : d : _) <- pure (map (* 10) lz)
    print (p + tot + fld + go + s + d)
  where
    go q = q + 1

comprehensions :: Int -> IO ()
comprehensions p = do
    d <- pure (p + 5)
    let s = p * 2
    print (go s + d + p)
    let lz = lazies p
    print [p + tot | (p, tot) <- zip lz (drop 1 lz)]
    print [fld + go | fld <- lz, let go = fld * 2, go > 10]
    print [s + d | (s, d) <- [lazyPair p]]
  where
    go q = q + 1

patternGuards :: Int -> IO ()
patternGuards p = do
    d <- pure (p + 5)
    let s = p * 2
    print (go s + d + p)
    print (pg (lazies p))
    print (pgLocal (lazies p))
    print (pgTop (lazies p))
  where
    go q = q + 1
    pg lz
        | (p : tot : _) <- lz, p > 100 = 0
        | (p : tot : fld : go : s : d : _) <- lz = p + tot + fld + go + s + d
        | otherwise = -1
    pgLocal lz
        | Just p <- safeHead lz, p > 1 = p * 10
        | otherwise = -1

pgTop :: [Int] -> Int
pgTop lz
    | Just tot <- safeHead (drop 1 lz), (fld : _) <- drop 2 lz = tot * fld
    | otherwise = -1

asNested :: Int -> IO ()
asNested p = do
    d <- pure (p + 5)
    let s = p * 2
    print (go s + d + p)
    case map Just [lazyPair p] of
        tot@(Just (p, fld) : _) -> print (p + fld + length tot)
        _ -> print (-1)
    case lazies p of
        s@(d : go : _) -> print (d + go + length s)
        _ -> print (-1)
  where
    go q = q + 1

fused :: Int -> IO ()
fused p = do
    d <- pure (p + 5)
    let s = p * 2
    print (go s + d + p)
    print (foldl' (\s p -> s + p * 2) 0 [1 .. p + 3])
    print (sum (map (\tot -> tot * 3) [1 .. p + 3]))
    print (length (filter (\fld -> fld > 2) [1 .. p + 3]))
    print (foldl' (\d go -> d + go) 0 (map (\tot -> tot + 1) [1 .. p + 3]))
  where
    go q = q + 1

clauseTop :: [Int] -> Int
clauseTop (tot : fld : _) = tot + fld
clauseTop _ = -1

clauseGuardedTop :: [Int] -> Int
clauseGuardedTop (tot : fld : _)
    | tot > 100 = 0
    | otherwise = tot * fld
clauseGuardedTop _ = -1

describe :: R -> String
describe (R fld tag) = tag <> " " <> show (fld + 1)

k :: Int -> [Int] -> String
k x ys = if x > 0
    then (case ys of
        (x : _) -> if x == 7 then "seven" else "other"
        [] -> "empty")
    else "neg"

pairUp :: [(Int, Int)] -> [Int]
pairUp ps = [tot + y | (tot, y) <- ps]

sumWith :: [Int] -> Int
sumWith xs = go xs
  where
    go (tot : rest) = tot + go rest
    go [] = 0

lam :: (Int, Int) -> Int
lam = \(tot, y) -> tot * y

nestedGroups :: [Int] -> Int
nestedGroups xs = go xs
  where
    go (v : rest) =
        let pick n [] = 0
            pick 0 (y : _) = y
            pick n (_ : ys) = pick (n - 1) ys
        in pick (v - 1) [v, v + 1] + go rest
    go [] = 0

nestedScrutinees :: [Int] -> [Int] -> Int
nestedScrutinees a b = case a of
    (x : _) | x > 0 -> case b of
        r | length r > 5 -> 0
        (y : _) -> y
        [] -> -1
    _ -> 99

main :: IO ()
main = do
    let n = length [(), ()] - 1
    caseValue n
    caseAction n
    caseGuarded n
    localClauses n
    lambdas n
    letForms n
    print (whereValues n (lazies n))
    doBinds n
    comprehensions n
    patternGuards n
    asNested n
    fused n
    print (clauseTop (lazies n))
    print (clauseGuardedTop (lazies n))
    putStrLn (describe (R (n + 30) ("x" <> show n)))
    putStrLn (k 1 [length [(), ()] + 5])
    print (pairUp [(n, 1), (n + 1, 2)])
    print (sumWith [n, n + 1])
    print (lam (n, n + 2))
    print (nestedGroups (map (\q -> q * 2) [1, 2]))
    print (nestedScrutinees [n] (map (+ 5) [n]))
    print (nestedScrutinees [n] (drop n [n]))
    print (nestedScrutinees [] [1])
