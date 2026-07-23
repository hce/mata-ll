-- Hindley-Milner type inference (Algorithm W): a self-checking stress test.
--
-- Target: the same machinery constraint solvers lean on -- unification with
-- an occurs check, substitution maps, and substitution composition -- plus a
-- recursive Ty/Term ADT flowing through inference at many positions (stresses
-- the monomorphizer), Either-based error plumbing, and Eq on a user type.
--
-- Programs are built directly as Term ASTs. The oracle renders each inferred
-- type with variables normalized to a, b, c... and asserts the string, so it
-- is robust to which fresh ids the engine happens to allocate.

-- Bitless FFI: turn an Int 0..25 into a letter via string.char.
chrFFI :: Int -> LuaPure "string.char" String

-- Types and terms.
data Ty = TVar Int | TInt | TBool | TFun Ty Ty
  deriving Eq

data Term = Var String
          | Lit Int
          | BLit Bool
          | Lam String Term
          | App Term Term
          | Let String Term Term

type Subst = [(Int, Ty)]
type Env   = [(String, Ty)]

-- ── lookups ───────────────────────────────────────────────────────────────

lookupSub :: Int -> Subst -> Maybe Ty
lookupSub _ [] = Nothing
lookupSub n ((k, v):rest) = if n == k then Just v else lookupSub n rest

lookupEnv :: String -> Env -> Maybe Ty
lookupEnv _ [] = Nothing
lookupEnv x ((k, v):rest) = if x == k then Just v else lookupEnv x rest

-- ── substitution ─────────────────────────────────────────────────────────

applyTy :: Subst -> Ty -> Ty
applyTy s (TVar n) = case lookupSub n s of
                       Just t  -> applyTy s t     -- resolve chains (acyclic by occurs check)
                       Nothing -> TVar n
applyTy _ TInt = TInt
applyTy _ TBool = TBool
applyTy s (TFun a b) = TFun (applyTy s a) (applyTy s b)

applyEnv :: Subst -> Env -> Env
applyEnv s env = map (\kv -> (fst kv, applyTy s (snd kv))) env

-- compose s2 s1 = apply s2 then s1 (s2 wins)
compose :: Subst -> Subst -> Subst
compose s2 s1 = map (\kv -> (fst kv, applyTy s2 (snd kv))) s1 ++ s2

-- ── unification ───────────────────────────────────────────────────────────

occurs :: Int -> Ty -> Bool
occurs n (TVar m) = n == m
occurs n (TFun a b) = occurs n a || occurs n b
occurs _ _ = False

bindVar :: Int -> Ty -> Either String Subst
bindVar n (TVar m) = if n == m then Right [] else Right [(n, TVar m)]
bindVar n t = if occurs n t then Left "occurs check" else Right [(n, t)]

unify :: Ty -> Ty -> Either String Subst
unify (TVar n) t = bindVar n t
unify t (TVar n) = bindVar n t
unify TInt TInt = Right []
unify TBool TBool = Right []
unify (TFun a1 b1) (TFun a2 b2) =
  case unify a1 a2 of
    Left e   -> Left e
    Right s1 -> case unify (applyTy s1 b1) (applyTy s1 b2) of
                  Left e   -> Left e
                  Right s2 -> Right (compose s2 s1)
unify _ _ = Left "type mismatch"

-- ── inference (Algorithm W, monomorphic let) ──────────────────────────────

-- State-passed fresh-variable counter; result threads (nextId, subst, type).
infer :: Int -> Env -> Term -> Either String (Int, Subst, Ty)
infer n env (Var x) =
  case lookupEnv x env of
    Just t  -> Right (n, [], t)
    Nothing -> Left ("unbound variable " <> x)
infer n _ (Lit _)  = Right (n, [], TInt)
infer n _ (BLit _) = Right (n, [], TBool)
infer n env (Lam x body) =
  let tv = TVar n
  in case infer (n + 1) ((x, tv) : env) body of
       Left e -> Left e
       Right (n1, s, tbody) -> Right (n1, s, TFun (applyTy s tv) tbody)
infer n env (App f a) =
  case infer n env f of
    Left e -> Left e
    Right (n1, s1, tf) ->
      case infer n1 (applyEnv s1 env) a of
        Left e -> Left e
        Right (n2, s2, ta) ->
          let tr = TVar n2
          in case unify (applyTy s2 tf) (TFun ta tr) of
               Left e   -> Left e
               Right s3 -> Right (n2 + 1, compose s3 (compose s2 s1), applyTy s3 tr)
infer n env (Let x e body) =
  case infer n env e of
    Left err -> Left err
    Right (n1, s1, te) ->
      case infer n1 ((x, te) : applyEnv s1 env) body of
        Left e -> Left e
        Right (n2, s2, tb) -> Right (n2, compose s2 s1, tb)

-- ── rendering (normalize tyvars to a, b, c...) ────────────────────────────

collectVars :: Ty -> [Int]
collectVars (TVar n) = [n]
collectVars TInt = []
collectVars TBool = []
collectVars (TFun a b) = collectVars a ++ collectVars b

nubInts :: [Int] -> [Int]
nubInts [] = []
nubInts (x:xs) = x : nubInts (filter (\y -> not (y == x)) xs)

indexOf :: Int -> [Int] -> Int
indexOf _ [] = 0
indexOf x (y:ys) = if x == y then 0 else 1 + indexOf x ys

letterFor :: Int -> String
letterFor i = chrFFI (97 + i)

renderWith :: [Int] -> Ty -> String
renderWith vars (TVar n) = letterFor (indexOf n vars)
renderWith _ TInt = "Int"
renderWith _ TBool = "Bool"
renderWith vars (TFun a b) = "(" <> renderWith vars a <> " -> " <> renderWith vars b <> ")"

showTy :: Ty -> String
showTy t = renderWith (nubInts (collectVars t)) t

-- Infer a closed term and render its normalized type, or the error.
typeOf :: Term -> String
typeOf tm = case infer 0 [] tm of
              Left e  -> "ERROR: " <> e
              Right r -> showTy (applyTy (snd3 r) (thd3 r))

snd3 :: (a, b, c) -> b
snd3 (_, b, _) = b

thd3 :: (a, b, c) -> c
thd3 (_, _, c) = c

-- ── sample terms ───────────────────────────────────────────────────────────

identity :: Term
identity = Lam "x" (Var "x")

konst :: Term
konst = Lam "x" (Lam "y" (Var "x"))

apply2 :: Term
apply2 = Lam "f" (Lam "x" (App (Var "f") (Var "x")))

idApplied :: Term
idApplied = App (Lam "x" (Var "x")) (Lit 42)

selfApp :: Term
selfApp = Lam "x" (App (Var "x") (Var "x"))

letTerm :: Term
letTerm = Let "i" (Lam "x" (Var "x")) (App (Var "i") (Lit 1))

main :: IO ()
main = do
  assert (typeOf identity == "(a -> a)")              "infer: identity :: a -> a"
  assert (typeOf konst == "(a -> (b -> a))")          "infer: const :: a -> b -> a"
  assert (typeOf apply2 == "((a -> b) -> (a -> b))")  "infer: apply :: (a->b) -> a -> b"
  assert (typeOf idApplied == "Int")                  "infer: (id 42) :: Int"
  assert (typeOf letTerm == "Int")                    "infer: let i = id in i 1 :: Int"
  -- self-application must fail the occurs check
  assert (typeOf selfApp == "ERROR: occurs check")    "infer: \\x -> x x fails occurs check"
  -- Eq on a user type (Ty) via deriving
  assert (TFun TInt TBool == TFun TInt TBool)         "Eq Ty: structural equality"
  assert (not (TInt == TBool))                        "Eq Ty: inequality"

  putStrLn ("identity : " <> typeOf identity)
  putStrLn ("const    : " <> typeOf konst)
  putStrLn ("apply    : " <> typeOf apply2)
  putStrLn "all type-inference checks passed"
