-- GHC cgrun062: Interpreter for a tiny language (variables, add, let)

data Expr = Lit Int | Var String | Add Expr Expr | MLet String Expr Expr
    deriving (Show, Eq)

lookupEnv :: String -> [(String, Int)] -> Maybe Int
lookupEnv _ [] = Nothing
lookupEnv k ((k2, v) : rest)
    | k == k2   = Just v
    | otherwise = lookupEnv k rest

eval :: [(String, Int)] -> Expr -> Maybe Int
eval _ (Lit n) = Just n
eval env (Var x) = lookupEnv x env
eval env (Add e1 e2) = eval env e1 >>= \v1 -> eval env e2 >>= \v2 -> Just (v1 + v2)
eval env (MLet x e body) = eval env e >>= \v -> eval ((x, v) : env) body

main :: IO ()
main = do
    assert (eval [] (Lit 42) == Just 42) "lit"
    assert (eval [("x", 10)] (Var "x") == Just 10) "var found"
    assert (eval [] (Var "y") == Nothing) "var not found"
    assert (eval [] (Add (Lit 3) (Lit 4)) == Just 7) "add"
    let e1 = MLet "x" (Lit 5) (Add (Var "x") (Lit 3))
    assert (eval [] e1 == Just 8) "let x=5 in x+3"
    let e2 = MLet "x" (Lit 2) (MLet "y" (Lit 3) (Add (Var "x") (Var "y")))
    assert (eval [] e2 == Just 5) "nested let"
    let e3 = MLet "x" (Lit 1) (MLet "x" (Lit 99) (Var "x"))
    assert (eval [] e3 == Just 99) "let shadowing"
    let e4 = Add (Lit 1) (Var "missing")
    assert (eval [] e4 == Nothing) "add with missing var"
    putStrLn "ok"
