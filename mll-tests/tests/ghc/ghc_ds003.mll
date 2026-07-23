-- GHC ds003: Nested case expressions
-- Tests case inside case, case in function body

data Expr = Num Int | Add Expr Expr | If Expr Expr Expr | IsZero Expr
    deriving (Show, Eq)

eval :: Expr -> Int
eval expr = case expr of
    Num n -> n
    Add a b -> eval a + eval b
    If cond t f -> case eval cond of
        0 -> eval f
        _ -> eval t
    IsZero e -> case eval e of
        0 -> 1
        _ -> 0

-- Quadrant classification
classify :: Int -> Int -> String
classify x y
    | x > 0 && y > 0 = "both positive"
    | x < 0 && y < 0 = "both negative"
    | x == 0 && y == 0 = "both zero"
    | x < 0 && y > 0 = "x neg, y pos"
    | x > 0 && y < 0 = "x pos, y neg"
    | x == 0 = "x zero"
    | otherwise = "y zero"

main :: IO ()
main = do
    assert (eval (Num 42) == 42) "eval num"
    assert (eval (Add (Num 3) (Num 4)) == 7) "eval add"
    assert (eval (If (Num 1) (Num 10) (Num 20)) == 10) "eval if true"
    assert (eval (If (Num 0) (Num 10) (Num 20)) == 20) "eval if false"
    assert (eval (IsZero (Num 0)) == 1) "eval iszero true"
    assert (eval (IsZero (Num 5)) == 0) "eval iszero false"

    -- Nested expression
    let e = If (IsZero (Add (Num 3) (Num (-3)))) (Num 100) (Num 200)
    assert (eval e == 100) "eval nested"

    assert (classify 5 3 == "both positive") "classify ++"
    assert (classify (-1) (-2) == "both negative") "classify --"
    assert (classify 0 0 == "both zero") "classify 00"
    assert (classify (-1) 5 == "x neg, y pos") "classify -+"

    putStrLn "ok"
