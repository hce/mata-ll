-- GHC cgrun016: Nested pattern matching
-- Tests pattern matching on ADTs with multiple levels

data Expr = Lit Integer | Add Expr Expr | Mul Expr Expr | Neg Expr
    deriving (Show, Eq)

eval :: Expr -> Integer
eval (Lit n)   = n
eval (Add a b) = eval a + eval b
eval (Mul a b) = eval a * eval b
eval (Neg e)   = 0 - eval e

simplify :: Expr -> Expr
simplify (Add (Lit 0) e) = simplify e
simplify (Add e (Lit 0)) = simplify e
simplify (Mul (Lit 0) _) = Lit 0
simplify (Mul _ (Lit 0)) = Lit 0
simplify (Mul (Lit 1) e) = simplify e
simplify (Mul e (Lit 1)) = simplify e
simplify (Neg (Neg e))   = simplify e
simplify (Add a b)       = Add (simplify a) (simplify b)
simplify (Mul a b)       = Mul (simplify a) (simplify b)
simplify (Neg e)         = Neg (simplify e)
simplify e               = e

main :: IO ()
main = do
    assert (eval (Add (Lit 3) (Mul (Lit 4) (Lit 5))) == 23) "eval"
    assert (eval (Neg (Lit 7)) == -7) "eval neg"
    assert (simplify (Add (Lit 0) (Lit 5)) == Lit 5) "simp add 0"
    assert (simplify (Mul (Lit 1) (Lit 7)) == Lit 7) "simp mul 1"
    assert (simplify (Mul (Lit 0) (Add (Lit 3) (Lit 4))) == Lit 0) "simp mul 0"
    assert (simplify (Neg (Neg (Lit 3))) == Lit 3) "simp double neg"
    putStrLn "ok"
