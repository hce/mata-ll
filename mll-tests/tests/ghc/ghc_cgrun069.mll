-- GHC cgrun069: Simple type inference (unification of simple types)

data Ty = TyInt | TyBool | TyFun Ty Ty | TyVar Integer
    deriving (Show, Eq)

applySubst :: [(Integer, Ty)] -> Ty -> Ty
applySubst _ TyInt  = TyInt
applySubst _ TyBool = TyBool
applySubst s (TyFun a b) = TyFun (applySubst s a) (applySubst s b)
applySubst s (TyVar n) = lookupVar n s

lookupVar :: Integer -> [(Integer, Ty)] -> Ty
lookupVar n [] = TyVar n
lookupVar n ((k, v):rest)
    | n == k    = applySubst rest v
    | otherwise = lookupVar n rest

occurs :: Integer -> Ty -> Bool
occurs n (TyVar m)   = n == m
occurs _ TyInt       = False
occurs _ TyBool      = False
occurs n (TyFun a b) = occurs n a || occurs n b

unify :: [(Integer, Ty)] -> Ty -> Ty -> Maybe [(Integer, Ty)]
unify s t1 t2 = unify_ s (applySubst s t1) (applySubst s t2)

unify_ :: [(Integer, Ty)] -> Ty -> Ty -> Maybe [(Integer, Ty)]
unify_ s TyInt TyInt   = Just s
unify_ s TyBool TyBool = Just s
unify_ s (TyVar n) t
    | TyVar n == t = Just s
    | occurs n t   = Nothing
    | otherwise    = Just ((n, t) : s)
unify_ s t (TyVar n)
    | occurs n t   = Nothing
    | otherwise    = Just ((n, t) : s)
unify_ s (TyFun a1 b1) (TyFun a2 b2) = unify s a1 a2 >>= \s2 -> unify s2 b1 b2
unify_ _ _ _ = Nothing

checkSubst :: Maybe [(Integer, Ty)] -> Integer -> Ty -> Bool
checkSubst Nothing _ _ = False
checkSubst (Just s) var expected = applySubst s (TyVar var) == expected

main :: IO ()
main = do
    assert (unify [] TyInt TyInt == Just []) "int ~ int"
    assert (unify [] TyInt TyBool == Nothing) "int ~ bool fails"
    let s1 = unify [] (TyVar 0) TyInt
    assert (s1 /= Nothing) "var ~ int succeeds"
    assert (checkSubst s1 0 TyInt) "var resolves"
    let s2 = unify [] (TyVar 0) (TyVar 1) >>= \s -> unify s (TyVar 1) TyBool
    assert (s2 /= Nothing) "chain succeeds"
    -- Var 1 resolves directly to TyBool
    assert (checkSubst s2 1 TyBool) "chain var1 resolves"
    assert (unify [] (TyVar 0) (TyFun (TyVar 0) TyInt) == Nothing) "occurs check"
    let s3 = unify [] (TyFun (TyVar 0) (TyVar 1)) (TyFun TyInt TyBool)
    assert (s3 /= Nothing) "fun unify succeeds"
    assert (checkSubst s3 0 TyInt) "fun arg"
    assert (checkSubst s3 1 TyBool) "fun res"
    putStrLn "ok"
