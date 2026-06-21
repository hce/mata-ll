-- Untyped lambda calculus with de Bruijn indices: a self-checking stress test.
--
-- Target: the laziness/thunk-forcing machinery (where most compiler bugs have
-- clustered). It is almost entirely a recursive Term ADT reduced lazily --
-- substitution, index shifting, normal-order reduction -- plus Maybe plumbing,
-- Integer arithmetic (including negative shifts), deep recursion, and
-- deriving Eq on a recursive type.
--
-- de Bruijn indices avoid variable capture: TVar n refers to the n-th
-- enclosing binder (0 = innermost). Oracle: reduce closed terms (identity,
-- boolean not, Church arithmetic) and assert the normal form structurally.

data Term = TVar Integer | TLam Term | TApp Term Term
  deriving Eq

-- Shift free indices (>= cutoff c) by d.
shift :: Integer -> Integer -> Term -> Term
shift d c (TVar n)   = if n >= c then TVar (n + d) else TVar n
shift d c (TLam t)   = TLam (shift d (c + 1) t)
shift d c (TApp f a) = TApp (shift d c f) (shift d c a)

-- Substitute term s for index j.
subst :: Integer -> Term -> Term -> Term
subst j s (TVar n)   = if n == j then s else TVar n
subst j s (TLam t)   = TLam (subst (j + 1) (shift 1 0 s) t)
subst j s (TApp f a) = TApp (subst j s f) (subst j s a)

-- Beta-reduce (\.body) arg.
betaReduce :: Term -> Term -> Term
betaReduce body arg = shift (0 - 1) 0 (subst 0 (shift 1 0 arg) body)

-- One normal-order (leftmost-outermost) reduction step, if any.
step :: Term -> Maybe Term
step (TApp (TLam body) arg) = Just (betaReduce body arg)
step (TApp f a) =
  case step f of
    Just f' -> Just (TApp f' a)
    Nothing -> case step a of
                 Just a' -> Just (TApp f a')
                 Nothing -> Nothing
step (TLam t) =
  case step t of
    Just t' -> Just (TLam t')
    Nothing -> Nothing
step (TVar _) = Nothing

-- Reduce to normal form (fuel-bounded; some terms have no normal form).
normalize :: Integer -> Term -> Term
normalize 0    t = t
normalize fuel t =
  case step t of
    Just t' -> normalize (fuel - 1) t'
    Nothing -> t

nf :: Term -> Term
nf = normalize 100000

-- ── encodings ─────────────────────────────────────────────────────────────

tId :: Term
tId = TLam (TVar 0)

tTrue :: Term
tTrue = TLam (TLam (TVar 1))

tFalse :: Term
tFalse = TLam (TLam (TVar 0))

-- \b. b false true
tNot :: Term
tNot = TLam (TApp (TApp (TVar 0) tFalse) tTrue)

-- \p.\q. p q p
tAnd :: Term
tAnd = TLam (TLam (TApp (TApp (TVar 1) (TVar 0)) (TVar 1)))

-- Church numeral n = \f.\x. f^n x
churchBody :: Integer -> Term
churchBody 0 = TVar 0
churchBody n = TApp (TVar 1) (churchBody (n - 1))

church :: Integer -> Term
church n = TLam (TLam (churchBody n))

-- \n.\f.\x. f (n f x)
tSucc :: Term
tSucc = TLam (TLam (TLam (TApp (TVar 1) (TApp (TApp (TVar 2) (TVar 1)) (TVar 0)))))

-- \m.\n.\f.\x. m f (n f x)
tPlus :: Term
tPlus = TLam (TLam (TLam (TLam (TApp (TApp (TVar 3) (TVar 1)) (TApp (TApp (TVar 2) (TVar 1)) (TVar 0))))))

-- \m.\n.\f.\x. m (n f) x
tMult :: Term
tMult = TLam (TLam (TLam (TLam (TApp (TApp (TVar 3) (TApp (TVar 2) (TVar 1))) (TVar 0)))))

ap2 :: Term -> Term -> Term -> Term
ap2 f a b = TApp (TApp f a) b

main :: IO ()
main = do
  -- identity applied to identity is identity
  assert (nf (TApp tId tId) == tId) "lambda: (\\x.x)(\\x.x) = \\x.x"
  -- boolean not
  assert (nf (TApp tNot tTrue) == tFalse) "lambda: not true = false"
  assert (nf (TApp tNot tFalse) == tTrue) "lambda: not false = true"
  -- and
  assert (nf (ap2 tAnd tTrue tTrue) == tTrue) "lambda: and true true = true"
  assert (nf (ap2 tAnd tTrue tFalse) == tFalse) "lambda: and true false = false"
  -- Church arithmetic, normal forms compared structurally
  assert (nf (TApp tSucc (church 0)) == church 1) "lambda: succ 0 = 1"
  assert (nf (ap2 tPlus (church 2) (church 3)) == church 5) "lambda: 2 + 3 = 5"
  assert (nf (ap2 tMult (church 2) (church 3)) == church 6) "lambda: 2 * 3 = 6"
  assert (nf (ap2 tMult (church 3) (church 3)) == church 9) "lambda: 3 * 3 = 9"

  putStrLn "all lambda-calculus checks passed"
