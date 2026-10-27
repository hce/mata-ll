-- A pattern that forces its scrutinee forces it in the clause condition,
-- at its left-to-right position, whether or not it tests or binds
-- anything. Forcing used to ride on the tests and the bindings alone: a
-- tuple, a single-constructor record or a LuaDict record pushes no test,
-- and a field path is evaluated only by a binding the body uses, so a
-- wildcard-only tuple or record (`g (Just (_, _))` on `Just ⊥`, `h (P _
-- _ : _)` on `[⊥]`, `k (_, (_, _))` on `(1, ⊥)`) and a tuple whose
-- bindings sat behind a later failing column (`f2 _ (a, b) True` on `f2
-- False ⊥ False`) were never forced where GHC raises. Now a no-test
-- forcing pattern contributes its force to the condition chain as an
-- always-true conjunct, so the chain alone reproduces GHC's match order:
-- a column failing to the left still short-circuits the forces to its
-- right, a forcing pattern with no bindings still diverges on ⊥, and a
-- tuple nested under a later failing column is forced before that column
-- is tried. Newtype patterns, `let`/`where` pattern bindings and
-- unforced lambda/case binders stay lazy. Shapes: the five of the report,
-- wildcard-only tuple, record and newtype columns, tuple-in-Just,
-- tuple-in-cons and record-in-tuple, an unused tuple binding, a tuple
-- column left of a failing column with and without a failing column to
-- its own left, the same in a where-local group, a guarded clause, a
-- value-position case, a comprehension generator, a lambda, a do-bind and
-- a let pattern binding. Found by the 2026-10 fresh-eyes review (O6).
-- expect: raised
-- expect: 5
-- expect: 0
-- expect: raised
-- expect: 1
-- expect: 3
-- expect: 0
-- expect: raised
-- expect: 1
-- expect: 0
-- expect: raised
-- expect: raised
-- expect: 1
-- expect: 1
-- expect: raised
-- expect: 2
-- expect: raised
-- expect: raised
-- expect: 7
-- expect: 7
-- expect: 7
-- expect: raised
-- expect: 0
-- expect: 1
-- expect: raised
-- expect: raised
-- expect: 1
-- expect: raised
-- expect: 0
-- expect: 1
-- expect: raised
-- expect: 2
-- expect: 1
-- expect: raised
-- expect: 2
-- expect: 2
-- expect: 1
-- expect: raised
-- expect: 2
-- expect: 2
-- expect: 1
-- expect: raised
-- expect: 2
-- expect: 1
-- expect: raised
-- expect: 1
-- expect: raised
-- expect: 1
-- expect: raised
-- expect: 1
-- expect: 1
-- expect: raised
-- expect: 3
-- expect: 1
-- expect: 1

data P = P Int Int

newtype N = N Int

report :: Int -> IO ()
report v = do
    r <- try (v `seq` pure v)
    putStrLn (either (const "raised") show r)

g :: Maybe (Int, Int) -> Int
g (Just (_, _)) = 5
g Nothing = 0

f2 :: Bool -> (Int, Int) -> Bool -> Int
f2 True _ _ = 1
f2 _ (a, b) True = a
f2 _ _ _ = 0

h :: [P] -> Int
h (P _ _ : _) = 1
h _ = 0

k :: (Int, (Int, Int)) -> Int
k (_, (_, _)) = 1

c :: Bool -> (Int, Int) -> Int
c b t = case (b, t) of
    (True, _) -> 1
    (_, (_, _)) -> 2

wt :: (Int, Int) -> Int
wt (_, _) = 7

wr :: P -> Int
wr (P _ _) = 7

wn :: N -> Int
wn (N _) = 7

wn2 :: N -> Int
wn2 (N x) = 7

tc :: [(Int, Int)] -> Int
tc ((_, _) : _) = 1
tc _ = 0

rt :: (Int, P) -> Int
rt (_, P _ _) = 1

ub :: Maybe Int -> (Int, Int) -> Int
ub (Just _) (a, b) = 1
ub _ _ = 0

lf2 :: Maybe Int -> (Int, Int) -> Bool -> Int
lf2 (Just 0) _ _ = 0
lf2 _ (_, _) True = 1
lf2 _ _ _ = 2

lf3 :: Maybe Int -> (Int, Int) -> Bool -> Int
lf3 (Just 0) _ _ = 0
lf3 (Just _) (_, _) True = 1
lf3 _ _ _ = 2

wl :: Maybe Int -> (Int, Int) -> Bool -> Int
wl x y z = go x y z
  where
    go (Just 0) _ _ = 0
    go (Just _) (_, _) True = 1
    go _ _ _ = 2

gc :: Maybe Int -> (Int, Int) -> Int
gc (Just 0) _ = 0
gc _ (_, _)
    | False = 1
gc _ _ = 2

vc :: (Int, Int) -> Int
vc t = case t of
    (_, _) -> 1

cg :: [Maybe (Int, Int)] -> Int
cg xs = sum [1 | Just (_, _) <- xs]

lam :: (Int, Int) -> Int
lam = \(_, _) -> 1

lamN :: N -> Int
lamN = \(N _) -> 1

dp :: (Int, Int) -> IO Int
dp t = do
    (_, _) <- pure t
    pure 3

lp :: (Int, Int) -> Int
lp t = let (a, b) = t in 1

wp :: (Int, Int) -> Int
wp t = 1
  where
    (a, b) = t

main :: IO ()
main = do
    report (g (Just (error "g payload")))
    report (g (Just (1, 2)))
    report (g Nothing)
    report (f2 False (error "f2 tuple") False)
    report (f2 True (error "f2 tuple") (error "f2 flag"))
    report (f2 False (3, 4) True)
    report (f2 False (3, 4) False)
    report (h [error "h head"])
    report (h [P 1 2])
    report (h [])
    report (k (1, error "k inner"))
    report (k (error "k outer"))
    report (k (1, (2, 3)))
    report (c True (error "c tuple"))
    report (c False (error "c tuple"))
    report (c False (1, 2))
    report (wt (error "wt"))
    report (wr (error "wr"))
    report (wt (1, 2))
    report (wn (error "wn"))
    report (wn2 (error "wn2"))
    report (tc [error "tc head"])
    report (tc [])
    report (tc [(1, 2)])
    report (rt (1, error "rt record"))
    report (rt (error "rt tuple"))
    report (rt (1, P 2 3))
    report (ub (Just 1) (error "ub tuple"))
    report (ub Nothing (error "ub tuple"))
    report (ub (Just 1) (2, 3))
    report (lf2 Nothing (error "lf2 tuple") False)
    report (lf2 Nothing (1, 2) False)
    report (lf2 Nothing (1, 2) True)
    report (lf3 (Just 1) (error "lf3 tuple") False)
    report (lf3 Nothing (error "lf3 tuple") False)
    report (lf3 (Just 1) (1, 2) False)
    report (lf3 (Just 1) (1, 2) True)
    report (wl (Just 1) (error "wl tuple") False)
    report (wl Nothing (error "wl tuple") False)
    report (wl (Just 1) (1, 2) False)
    report (wl (Just 1) (1, 2) True)
    report (gc Nothing (error "gc tuple"))
    report (gc Nothing (1, 2))
    report (vc (1, 2))
    report (vc (error "vc"))
    report (cg [Nothing, Just (1, 2)])
    report (cg [Nothing, Just (error "cg payload")])
    report (lam (1, 2))
    report (lam (error "lam"))
    report (lamN (error "lamN"))
    report (lamN (N 1))
    r <- try (dp (error "dp tuple"))
    putStrLn (either (const "raised") show r)
    r2 <- try (dp (1, 2))
    putStrLn (either (const "raised") show r2)
    report (lp (error "lp"))
    report (wp (error "wp"))
