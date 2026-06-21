-- A tiny Scheme evaluator: a self-checking compiler stress test.
--
-- Target of this stress test: the MONOMORPHIZER. A single recursive Value sum
-- type flows through eval/apply/environments at many positions ([Value], Env,
-- closures capturing Env, primitive dispatch), alongside a recursive Expr AST.
-- That is exactly the shape that forces the monomorphizer to specialize one
-- type at many call sites.
--
-- Programs are built directly as Expr ASTs (no textual reader) so the pressure
-- stays on evaluation rather than string parsing. Recursion is expressed by
-- self-application (passing a lambda to itself), so the interpreter needs no
-- mutable/recursive environment.
--
-- Oracle: each program is evaluated and its result asserted against the known
-- answer; a wrong result -> assert -> error -> the program (and test) fails.

-- Runtime values.
data Value = VNum Integer
           | VBool Bool
           | VNil
           | VCons Value Value
           | VClosure [String] Expr Env
           | VPrim String

-- Expression AST.
data Expr = ENum Integer
          | EBool Bool
          | EVar String
          | EIf Expr Expr Expr
          | ELambda [String] Expr
          | ELet [(String, Expr)] Expr
          | EApp Expr [Expr]

-- Environment as a chained association list (environment chaining).
data Env = EnvNil | EnvCons String Value Env

-- ── environment ──────────────────────────────────────────────────────────

envLookup :: String -> Env -> Value
envLookup x EnvNil = error ("scheme: unbound variable " <> x)
envLookup x (EnvCons k v rest) = if x == k then v else envLookup x rest

bindParams :: [String] -> [Value] -> Env -> Env
bindParams []     []     env = env
bindParams (p:ps) (a:as) env = bindParams ps as (EnvCons p a env)
bindParams _      _      _   = error "scheme: arity mismatch"

baseEnv :: Env
baseEnv = foldl (\acc n -> EnvCons n (VPrim n) acc) EnvNil ["+", "-", "*", "=", "<", "cons", "car", "cdr", "null?"]

-- ── evaluator ──────────────────────────────────────────────────────────────

eval :: Expr -> Env -> Value
eval (ENum n)        _   = VNum n
eval (EBool b)       _   = VBool b
eval (EVar x)        env = envLookup x env
eval (EIf c t e)     env = case eval c env of
  VBool False -> eval e env
  _           -> eval t env       -- non-#f is truthy (Scheme semantics)
eval (ELambda ps body) env = VClosure ps body env
eval (ELet binds body) env =
  let env' = foldl (\acc bnd -> EnvCons (fst bnd) (eval (snd bnd) env) acc) env binds
  in eval body env'
eval (EApp f args) env = apply (eval f env) (map (\a -> eval a env) args)

apply :: Value -> [Value] -> Value
apply (VClosure ps body env) args = eval body (bindParams ps args env)
apply (VPrim name)           args = applyPrim name args
apply _                      _    = error "scheme: application of non-function"

applyPrim :: String -> [Value] -> Value
applyPrim "+"     [VNum a, VNum b] = VNum (a + b)
applyPrim "-"     [VNum a, VNum b] = VNum (a - b)
applyPrim "*"     [VNum a, VNum b] = VNum (a * b)
applyPrim "="     [VNum a, VNum b] = VBool (a == b)
applyPrim "<"     [VNum a, VNum b] = VBool (a < b)
applyPrim "cons"  [a, b]           = VCons a b
applyPrim "car"   [VCons a _]      = a
applyPrim "cdr"   [VCons _ b]      = b
applyPrim "null?" [VNil]           = VBool True
applyPrim "null?" [_]              = VBool False
applyPrim name    _                = error ("scheme: bad primitive call " <> name)

-- ── result extraction ──────────────────────────────────────────────────────

asNum :: Value -> Integer
asNum (VNum n) = n
asNum _        = error "scheme: expected a number"

run :: Expr -> Value
run e = eval e baseEnv

-- ── program builders (readability helpers) ─────────────────────────────────

prim2 :: String -> Expr -> Expr -> Expr
prim2 op a b = EApp (EVar op) [a, b]

-- ── sample programs ─────────────────────────────────────────────────────────

-- (+ (* 2 3) 4) = 10
arith :: Expr
arith = prim2 "+" (prim2 "*" (ENum 2) (ENum 3)) (ENum 4)

-- (if (< 1 2) 10 20) = 10
cond :: Expr
cond = EIf (prim2 "<" (ENum 1) (ENum 2)) (ENum 10) (ENum 20)

-- ((lambda (x) (* x x)) 5) = 25
square5 :: Expr
square5 = EApp (ELambda ["x"] (prim2 "*" (EVar "x") (EVar "x"))) [ENum 5]

-- (let ((x 3) (y 4)) (+ x y)) = 7
letAdd :: Expr
letAdd = ELet [("x", ENum 3), ("y", ENum 4)] (prim2 "+" (EVar "x") (EVar "y"))

-- (((lambda (x) (lambda (y) (+ x y))) 3) 4) = 7  -- closure captures x
curried :: Expr
curried = EApp (EApp (ELambda ["x"] (ELambda ["y"] (prim2 "+" (EVar "x") (EVar "y")))) [ENum 3]) [ENum 4]

-- factorial via self-application:
-- ((lambda (self n) (if (= n 0) 1 (* n (self self (- n 1))))) <itself> 5) = 120
factLam :: Expr
factLam = ELambda ["self", "n"]
            (EIf (prim2 "=" (EVar "n") (ENum 0))
                 (ENum 1)
                 (prim2 "*" (EVar "n")
                        (EApp (EVar "self") [EVar "self", prim2 "-" (EVar "n") (ENum 1)])))

factOf :: Integer -> Expr
factOf k = EApp factLam [factLam, ENum k]

-- sum of a cons-list 1..n, also via self-application
sumLam :: Expr
sumLam = ELambda ["self", "lst"]
           (EIf (EApp (EVar "null?") [EVar "lst"])
                (ENum 0)
                (prim2 "+" (EApp (EVar "car") [EVar "lst"])
                       (EApp (EVar "self") [EVar "self", EApp (EVar "cdr") [EVar "lst"]])))

-- build the list (cons 1 (cons 2 (cons 3 nil)))
list123 :: Expr
list123 = EApp (EVar "cons") [ENum 1, EApp (EVar "cons") [ENum 2, EApp (EVar "cons") [ENum 3, EVar "nil"]]]

main :: IO ()
main = do
  assert (asNum (run arith)    == 10)  "scheme: (+ (* 2 3) 4) == 10"
  assert (asNum (run cond)     == 10)  "scheme: (if (< 1 2) 10 20) == 10"
  assert (asNum (run square5)  == 25)  "scheme: ((lambda (x) (* x x)) 5) == 25"
  assert (asNum (run letAdd)   == 7)   "scheme: let binding == 7"
  assert (asNum (run curried)  == 7)   "scheme: curried closure capture == 7"
  assert (asNum (run (factOf 5))  == 120) "scheme: factorial 5 == 120"
  assert (asNum (run (factOf 10)) == 3628800) "scheme: factorial 10 == 3628800"

  -- evaluate (sum (list 1 2 3)) = 6 in an env where `nil` is bound to VNil
  let env2 = EnvCons "nil" VNil baseEnv
  let sumExpr = EApp sumLam [sumLam, list123]
  assert (asNum (eval sumExpr env2) == 6) "scheme: sum of (1 2 3) == 6"

  putStrLn ("factorial 10 = " <> show (asNum (run (factOf 10))))
  putStrLn "all scheme evaluator checks passed"
