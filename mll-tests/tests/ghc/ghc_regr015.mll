-- ghc_regr015: GADT: type-safe expression evaluator

data Expr a where
    LitI  :: Integer -> Expr Integer
    LitB  :: Bool -> Expr Bool
    LitS  :: String -> Expr String
    AddE  :: Expr Integer -> Expr Integer -> Expr Integer
    MulE  :: Expr Integer -> Expr Integer -> Expr Integer
    NegE  :: Expr Integer -> Expr Integer
    AndE  :: Expr Bool -> Expr Bool -> Expr Bool
    OrE   :: Expr Bool -> Expr Bool -> Expr Bool
    NotE  :: Expr Bool -> Expr Bool
    IfE   :: Expr Bool -> Expr a -> Expr a -> Expr a
    EqI   :: Expr Integer -> Expr Integer -> Expr Bool
    AppendE :: Expr String -> Expr String -> Expr String

evalI :: Expr Integer -> Integer
evalI (LitI n)    = n
evalI (AddE a b)  = evalI a + evalI b
evalI (MulE a b)  = evalI a * evalI b
evalI (NegE a)    = 0 - evalI a
evalI (IfE c t f) = if evalB c then evalI t else evalI f

evalB :: Expr Bool -> Bool
evalB (LitB b)    = b
evalB (AndE a b)  = evalB a && evalB b
evalB (OrE a b)   = evalB a || evalB b
evalB (NotE a)    = not (evalB a)
evalB (EqI a b)   = evalI a == evalI b
evalB (IfE c t f) = if evalB c then evalB t else evalB f

evalS :: Expr String -> String
evalS (LitS s)      = s
evalS (AppendE a b) = evalS a ++ evalS b
evalS (IfE c t f)   = if evalB c then evalS t else evalS f

main :: IO ()
main = do
    -- Integer expressions
    assert (evalI (LitI 42) == 42) "lit 42"
    assert (evalI (AddE (LitI 3) (LitI 4)) == 7) "add"
    assert (evalI (MulE (LitI 6) (LitI 7)) == 42) "mul"
    assert (evalI (NegE (LitI 5)) == (-5)) "neg"
    assert (evalI (AddE (MulE (LitI 2) (LitI 3)) (LitI 4)) == 10) "add mul"

    -- Bool expressions
    assert (evalB (LitB True) == True) "litB true"
    assert (evalB (NotE (LitB True)) == False) "not true"
    assert (evalB (AndE (LitB True) (LitB False)) == False) "and"
    assert (evalB (OrE (LitB False) (LitB True)) == True) "or"

    -- Integer equality as Bool
    assert (evalB (EqI (LitI 3) (LitI 3)) == True) "eqi same"
    assert (evalB (EqI (LitI 3) (LitI 4)) == False) "eqi diff"

    -- IfE on Integer
    assert (evalI (IfE (LitB True) (LitI 1) (LitI 2)) == 1) "ifE true I"
    assert (evalI (IfE (LitB False) (LitI 1) (LitI 2)) == 2) "ifE false I"

    -- IfE on Bool
    assert (evalB (IfE (LitB True) (LitB False) (LitB True)) == False) "ifE true B"

    -- String expressions
    assert (evalS (LitS "hello") == "hello") "litS"
    assert (evalS (AppendE (LitS "foo") (LitS "bar")) == "foobar") "append"
    assert (evalS (IfE (EqI (LitI 1) (LitI 1)) (LitS "yes") (LitS "no")) == "yes") "ifE string"

    -- Nested
    let expr = IfE (EqI (AddE (LitI 2) (LitI 3)) (LitI 5)) (MulE (LitI 6) (LitI 7)) (LitI 0)
    assert (evalI expr == 42) "complex GADT expr"

    putStrLn "ok"
