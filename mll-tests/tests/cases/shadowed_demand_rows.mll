-- Structured demand rows vs. shadowing (F2). Two shapes that once judged a
-- binding demanded through a NAME whose call site resolves to a different
-- binder, evaluating `boomA`/`boomC` eagerly — a spurious bottom GHC never
-- touches:
--   fA: a case binder shadows a strict where-local `go`; the structured
--       local_fn_rows carried no rebound filter (its boolean twin did), so
--       go's row applied to a call that targets the BINDER (= pickA).
--   fC: a case binder shadows the strict TOP-LEVEL `inc`; codegen's
--       demanded-map entry had no clause-binder masking, so the global row
--       applied the same way.
-- Controls fB/fD pin that genuine demand through the same shapes still
-- flows (results computed, not over-masked).
module Main where

inc :: Int -> Int
inc n = n + 1

fA :: Int -> Int -> Int
fA x y = case pickA of
    go -> go boomA
  where
    go n = n + 1
    boomA = y + 1
    pickA = \_ -> 0
    unused = go x

fC :: Int -> Int -> Int
fC x y = case pickC of
    inc -> inc boomC
  where
    boomC = y + 1
    pickC = \_ -> 0

-- Control: same where-local, binder does NOT collide; go's row applies to
-- the real go, and `deep` is demanded through it.
fB :: Int -> Int -> Int
fB x y = case pickB of
    h -> go deep
  where
    go n = n + 1
    deep = y + 1
    pickB = \_ -> 0

-- Control: plain demanded where-binding.
fD :: Int -> Int -> Int
fD _ y = go deep
  where
    go n = n + 1
    deep = y + 1

main :: IO ()
main = do
  print (fA 7 (error "boomA"))
  print (fC 7 (error "boomC"))
  print (fB 7 2)
  print (fD 7 2)

-- expect: 0
-- expect: 0
-- expect: 4
-- expect: 4
