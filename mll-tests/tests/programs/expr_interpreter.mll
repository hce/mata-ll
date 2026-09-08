-- A small expression language: a tokenizer over character codes, a
-- precedence-climbing parser, and an evaluator with lexical closures,
-- all with Either-typed errors threaded through do-notation.
-- Leans on: the Either monad, deep derived Show (nested constructors,
-- negative Integer fields), Data.Map environments, guard fall-through in
-- case alternatives, self-application for recursion in the object language.
import LString
import Data.Map (Map)
import qualified Data.Map as M

data Token
    = TNum Integer
    | TIdent String
    | TOp String
    | TLParen
    | TRParen
    | TLet
    | TIn
    | TEq
    | TIf
    | TThen
    | TElse
    | TLambda
    | TArrow
    deriving (Show, Eq)

data Expr
    = Num Integer
    | Var String
    | BinOp String Expr Expr
    | Let String Expr Expr
    | If Expr Expr Expr
    | Lam String Expr
    | App Expr Expr
    deriving (Show, Eq)

data Value
    = VInt Integer
    | VBool Bool
    | VClosure String Expr (Map String Value)

instance Show Value where
    show (VInt n) = show n
    show (VBool b) = show b
    show (VClosure x _ _) = "<closure \\" <> x <> ">"

type Env = Map String Value

isDigit :: Int -> Bool
isDigit c = c >= 48 && c <= 57

isLetter :: Int -> Bool
isLetter c = c >= 97 && c <= 122

isSpace :: Int -> Bool
isSpace c = c == 32 || c == 10 || c == 9

fromCodes :: [Int] -> String
fromCodes cs = mconcat (map strChar cs)

keyword :: String -> Token
keyword "let" = TLet
keyword "in" = TIn
keyword "if" = TIf
keyword "then" = TThen
keyword "else" = TElse
keyword w = TIdent w

tokenize :: [Int] -> Either String [Token]
tokenize [] = Right []
tokenize (c : cs)
    | isSpace c = tokenize cs
    | isDigit c =
        let (ds, rest) = span isDigit (c : cs)
            n = foldl (\acc d -> acc * 10 + toInteger (d - 48)) 0 ds
        in fmap (\ts -> TNum n : ts) (tokenize rest)
    | isLetter c =
        let (ls, rest) = span isLetter (c : cs)
        in fmap (\ts -> keyword (fromCodes ls) : ts) (tokenize rest)
    | c == 40 = fmap (\ts -> TLParen : ts) (tokenize cs)
    | c == 41 = fmap (\ts -> TRParen : ts) (tokenize cs)
    | c == 92 = fmap (\ts -> TLambda : ts) (tokenize cs)
    | c == 45 = case cs of
        (62 : rest) -> fmap (\ts -> TArrow : ts) (tokenize rest)
        _ -> fmap (\ts -> TOp "-" : ts) (tokenize cs)
    | c == 61 = case cs of
        (61 : rest) -> fmap (\ts -> TOp "==" : ts) (tokenize rest)
        _ -> fmap (\ts -> TEq : ts) (tokenize cs)
    | c == 43 || c == 42 || c == 47 || c == 60 || c == 62 =
        fmap (\ts -> TOp (strChar c) : ts) (tokenize cs)
    | otherwise = Left ("unexpected character code " <> show c)

prec :: String -> Int
prec "==" = 1
prec "<" = 1
prec ">" = 1
prec "+" = 2
prec "-" = 2
prec "*" = 3
prec "/" = 3
prec _ = 0

type Parse a = Either String (a, [Token])

expect :: Token -> [Token] -> Either String [Token]
expect t (t' : rest) | t == t' = Right rest
expect t ts = Left ("expected " <> show t <> " but found " <> show (take 1 ts))

parseExpr :: [Token] -> Parse Expr
parseExpr (TLet : TIdent x : TEq : rest) = do
    (bound, afterBound) <- parseExpr rest
    afterIn <- expect TIn afterBound
    (body, afterBody) <- parseExpr afterIn
    Right (Let x bound body, afterBody)
parseExpr (TIf : rest) = do
    (c, afterC) <- parseExpr rest
    afterThen <- expect TThen afterC
    (t, afterT) <- parseExpr afterThen
    afterElse <- expect TElse afterT
    (e, afterE) <- parseExpr afterElse
    Right (If c t e, afterE)
parseExpr (TLambda : TIdent x : TArrow : rest) = do
    (body, afterBody) <- parseExpr rest
    Right (Lam x body, afterBody)
parseExpr ts = parseBinary 1 ts

parseBinary :: Int -> [Token] -> Parse Expr
parseBinary minPrec ts = do
    (lhs, rest) <- parseApp ts
    climb lhs rest
  where
    climb lhs ts' = case ts' of
        (TOp op : more) | prec op >= minPrec -> do
            (rhs, rest') <- parseBinary (prec op + 1) more
            climb (BinOp op lhs rhs) rest'
        _ -> Right (lhs, ts')

-- Juxtaposition is application, left-associative and tightest.
parseApp :: [Token] -> Parse Expr
parseApp ts = do
    (f, rest) <- parseAtom ts
    gather f rest
  where
    gather f ts' = case ts' of
        (t : _) | startsAtom t -> do
            (a, rest') <- parseAtom ts'
            gather (App f a) rest'
        _ -> Right (f, ts')

startsAtom :: Token -> Bool
startsAtom (TNum _) = True
startsAtom (TIdent _) = True
startsAtom TLParen = True
startsAtom _ = False

parseAtom :: [Token] -> Parse Expr
parseAtom (TNum n : rest) = Right (Num n, rest)
parseAtom (TIdent x : rest) = Right (Var x, rest)
parseAtom (TLParen : rest) = do
    (e, afterE) <- parseExpr rest
    afterParen <- expect TRParen afterE
    Right (e, afterParen)
parseAtom ts = Left ("unexpected " <> show (take 1 ts))

parseProgram :: String -> Either String Expr
parseProgram src = do
    ts <- tokenize (strToInts src)
    (e, rest) <- parseExpr ts
    case rest of
        [] -> Right e
        _ -> Left ("trailing tokens " <> show rest)

binop :: String -> Value -> Value -> Either String Value
binop op (VInt a) (VInt b)
    | op == "+" = Right (VInt (a + b))
    | op == "-" = Right (VInt (a - b))
    | op == "*" = Right (VInt (a * b))
    | op == "/" = if b == 0 then Left "division by zero" else Right (VInt (a `div` b))
    | op == "<" = Right (VBool (a < b))
    | op == ">" = Right (VBool (a > b))
    | op == "==" = Right (VBool (a == b))
    | otherwise = Left ("unknown operator " <> op)
binop op (VBool a) (VBool b)
    | op == "==" = Right (VBool (a == b))
    | otherwise = Left ("operator " <> op <> " needs integers")
binop op _ _ = Left ("operator " <> op <> " applied to mismatched operands")

eval :: Env -> Expr -> Either String Value
eval _ (Num n) = Right (VInt n)
eval env (Var x) = case M.lookup x env of
    Nothing -> Left ("unbound variable " <> x)
    Just v -> Right v
eval env (BinOp op a b) = do
    va <- eval env a
    vb <- eval env b
    binop op va vb
eval env (Let x e body) = do
    v <- eval env e
    eval (M.insert x v env) body
eval env (If c t e) = do
    vc <- eval env c
    case vc of
        VBool True -> eval env t
        VBool False -> eval env e
        VInt _ -> Left "if: condition is not a boolean"
        VClosure _ _ _ -> Left "if: condition is not a boolean"
eval env (Lam x body) = Right (VClosure x body env)
eval env (App f a) = do
    vf <- eval env f
    va <- eval env a
    case vf of
        VClosure x body closureEnv -> eval (M.insert x va closureEnv) body
        VInt _ -> Left "application of a non-function"
        VBool _ -> Left "application of a non-function"

programs :: [String]
programs =
    [ "1 + 2 * 3 - 4 / 2"
    , "(1 + 2) * (3 - 4) / 2"
    , "let x = 5 in let y = x * 2 in x + y"
    , "if 3 < 4 then 10 else 20"
    , "if 1 == 2 then 10 else 0 - 7"
    , "let twice = \\f -> \\x -> f (f x) in twice (\\n -> n * 3) 7"
    , "let fact = \\f -> \\n -> if n < 1 then 1 else n * f f (n - 1) in fact fact 20"
    , "let add = \\a -> \\b -> a + b in let inc = add 1 in inc (inc 40)"
    , "let x = 1 in y"
    , "10 / (5 - 5)"
    , "if 1 then 2 else 3"
    , "3 4"
    , "let x = in 3"
    , "1 + $"
    , "(1 + 2"
    , "let big = 1000000000 in big * big * big * big"
    , "1 == 1 == 1"
    ]

run :: String -> IO ()
run src = do
    putStrLn ("> " <> src)
    case parseProgram src of
        Left e -> putStrLn ("  parse error: " <> e)
        Right ast -> do
            putStrLn ("  ast: " <> show ast)
            case eval M.empty ast of
                Left e -> putStrLn ("  error: " <> e)
                Right v -> putStrLn ("  value: " <> show v)

main :: IO ()
main = mapM_ run programs
