-- Exact-first-force let/where eagerization (exact_demanded_bindings):
-- a binding whose force is provably the body's FIRST possibly-bottoming
-- act may be evaluated at binding time even when its RHS is an
-- expensive call. The pins here are semantic and GHC-goldened: values
-- must match byte-exactly, and — the sharp edge — bindings OFF the
-- first-force chain must stay lazy, so the `error` RHSes below must
-- never run. A wrong eagerization crashes and the golden catches it.
-- (The HashMap flavor of these shapes lives in exact_let_eager_hm.mll,
-- self-asserting — hm builtins are outside the GHC oracle.)
module Main where

-- Expensive on purpose: a real call, not structurally cheap, so only
-- the first-force proof can eagerize it.
tri :: Int -> Int
tri n = sum [1 .. n]

-- Var-scrutinee anchor: the case forces `x` first thing.
varAnchor :: Int -> Int
varAnchor n =
    let x = tri n
    in case x of
        0 -> -1
        v -> v * 2

-- A two-link chain: `s` anchors through the scrutinee, `a` through
-- show's entry-forced argument.
showChain :: Int -> String
showChain n =
    let a = tri n
        s = show a
    in case s of
        "" -> "empty"
        r  -> r <> "!"

-- head's list argument is entry-forced; the map call moves to binding
-- time and still builds the same lazy spine.
headAnchor :: [Int] -> Int
headAnchor xs =
    let ys = map (* 2) xs
    in case head ys of
        0 -> -1
        h -> h

-- An if-condition anchors like a scrutinee; `not` is entry-forced.
notAnchor :: Int -> String
notAnchor n =
    let b = even (tri n)
    in if not b then "odd" else "even"

-- NOT on the chain: the scrutinee is a parameter, so `dead` must stay
-- lazy — the taken branch never forces it, and GHC never runs it.
offChain :: Int -> Int
offChain n =
    let dead = tri (error "offChain: must never run")
    in case n of
        0 -> dead
        v -> v

-- An unused expensive binding must never run at all.
unusedLazy :: Int
unusedLazy = let u = tri (error "unusedLazy: must never run") in 42

-- The where flavor of the chain (guard-free clause body anchors).
whereChain :: Int -> String
whereChain n = case s2 of
    "" -> "empty"
    r  -> r <> "."
  where
    s1 = tri n
    s2 = show s1

-- A guarded clause never anchors (the first guard runs before the
-- body), so `w` stays lazy and the taken guard never touches it.
guardedLazy :: Int -> Int
guardedLazy n
    | n > 0 = n
    | otherwise = w
  where
    w = tri (error "guardedLazy: must never run")

main :: IO ()
main = do
    print (varAnchor 100)
    putStrLn (showChain 37)
    print (headAnchor [21, 4])
    putStrLn (notAnchor 2)
    putStrLn (notAnchor 3)
    print (offChain 5)
    print unusedLazy
    putStrLn (whereChain 4444)
    print (guardedLazy 9)
