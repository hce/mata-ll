-- A tiny Lisp: an S-expression reader over character codes, an
-- environment-passing evaluator with closures, define, if, arithmetic,
-- list primitives, and a driver that threads global definitions through
-- a sequence of forms.
-- Leans on: mutually recursive parser functions, a recursive value type
-- with closures capturing a Data.Map environment, Either error handling
-- with do-notation, custom Show, deep recursion (factorial, fib, map).
import LString
import Data.Map (Map)
import qualified Data.Map as M

data SExpr
    = SNum Integer
    | SSym String
    | SList [SExpr]
    | SLambda [String] SExpr (Map String SExpr)
    | SPrim String
    | SBool Bool

instance Show SExpr where
    show (SNum n) = show n
    show (SSym s) = s
    show (SList xs) = "(" <> joinWords (map show xs) <> ")"
    show (SLambda ps _ _) = "<lambda/" <> show (length ps) <> ">"
    show (SPrim p) = "<prim " <> p <> ">"
    show (SBool True) = "#t"
    show (SBool False) = "#f"

joinWords :: [String] -> String
joinWords [] = ""
joinWords [w] = w
joinWords (w : ws) = w <> " " <> joinWords ws

type Env = Map String SExpr

isSpace :: Int -> Bool
isSpace c = c == 32 || c == 10 || c == 9

isDelim :: Int -> Bool
isDelim c = isSpace c || c == 40 || c == 41

isDigit :: Int -> Bool
isDigit c = c >= 48 && c <= 57

fromCodes :: [Int] -> String
fromCodes cs = mconcat (map strChar cs)

readExpr :: [Int] -> Either String (SExpr, [Int])
readExpr input = case dropWhile isSpace input of
    [] -> Left "unexpected end of input"
    (40 : rest) -> readList' [] rest
    (41 : _) -> Left "unexpected )"
    cs ->
        let (tok, rest) = span (not . isDelim) cs
        in Right (atom tok, rest)

readList' :: [SExpr] -> [Int] -> Either String (SExpr, [Int])
readList' acc input = case dropWhile isSpace input of
    [] -> Left "unterminated list"
    (41 : rest) -> Right (SList (reverse acc), rest)
    cs -> do
        (e, rest) <- readExpr cs
        readList' (e : acc) rest

atom :: [Int] -> SExpr
atom cs
    | not (null cs) && all isDigit cs = SNum (digits cs)
    | isNegative cs = SNum (negate (digits (tail cs)))
    | cs == [35, 116] = SBool True
    | cs == [35, 102] = SBool False
    | otherwise = SSym (fromCodes cs)
  where digits = foldl (\acc d -> acc * 10 + toInteger (d - 48)) 0

isNegative :: [Int] -> Bool
isNegative (45 : ds) = not (null ds) && all isDigit ds
isNegative _ = False

readAll :: String -> Either String [SExpr]
readAll src = go (strToInts src)
  where
    go cs = case dropWhile isSpace cs of
        [] -> Right []
        rest -> do
            (e, rest') <- readExpr rest
            es <- go rest'
            Right (e : es)

primitives :: Env
primitives = M.fromList [(p, SPrim p) | p <- ["+", "-", "*", "<", "=", "car", "cdr", "cons", "list", "null?"]]

eval :: Env -> SExpr -> Either String SExpr
eval _ (SNum n) = Right (SNum n)
eval _ (SBool b) = Right (SBool b)
eval env (SSym s) = case M.lookup s env of
    Nothing -> Left ("unbound symbol " <> s)
    Just v -> Right v
eval _ (SList []) = Right (SList [])
eval env (SList (SSym "quote" : args)) = case args of
    [x] -> Right x
    _ -> Left "quote takes one argument"
eval env (SList (SSym "if" : args)) = case args of
    [c, t, e] -> do
        cv <- eval env c
        case cv of
            SBool False -> eval env e
            _ -> eval env t
    _ -> Left "if takes three arguments"
eval env (SList (SSym "lambda" : args)) = case args of
    [SList params, body] -> do
        names <- mapM symbolName params
        Right (SLambda names body env)
    _ -> Left "malformed lambda"
eval env (SList (SSym "let" : args)) = case args of
    [SList bindings, body] -> do
        pairs <- mapM binding bindings
        vals <- mapM (\(_, e) -> eval env e) pairs
        eval (foldl (\m ((n, _), v) -> M.insert n v m) env (zip pairs vals)) body
    _ -> Left "malformed let"
eval env (SList (f : args)) = do
    fv <- eval env f
    argv <- mapM (eval env) args
    apply env fv argv
eval _ e = Left ("cannot evaluate " <> show e)

symbolName :: SExpr -> Either String String
symbolName (SSym s) = Right s
symbolName e = Left ("expected a symbol, got " <> show e)

binding :: SExpr -> Either String (String, SExpr)
binding (SList [SSym n, e]) = Right (n, e)
binding e = Left ("malformed binding " <> show e)

-- Globals defined so far are visible inside every closure, which is what
-- lets a defined function call itself.
apply :: Env -> SExpr -> [SExpr] -> Either String SExpr
apply globals (SLambda params body closure) args
    | length params /= length args = Left ("arity mismatch: expected " <> show (length params) <> ", got " <> show (length args))
    | otherwise = eval (M.union (M.fromList (zip params args)) (M.union closure globals)) body
apply _ (SPrim p) args = prim p args
apply _ f _ = Left ("not a function: " <> show f)

prim :: String -> [SExpr] -> Either String SExpr
prim "+" args = fmap (SNum . sum) (mapM number args)
prim "*" args = fmap (SNum . product) (mapM number args)
prim "-" args = do
    ns <- mapM number args
    case ns of
        [] -> Left "- needs an argument"
        [n] -> Right (SNum (negate n))
        (n : rest) -> Right (SNum (n - sum rest))
prim "<" [a, b] = do
    x <- number a
    y <- number b
    Right (SBool (x < y))
prim "=" [a, b] = do
    x <- number a
    y <- number b
    Right (SBool (x == y))
prim "car" [SList (x : _)] = Right x
prim "cdr" [SList (_ : xs)] = Right (SList xs)
prim "cons" [x, SList xs] = Right (SList (x : xs))
prim "list" args = Right (SList args)
prim "null?" [SList xs] = Right (SBool (null xs))
prim p args = Left ("bad arguments to " <> p <> ": " <> show (SList args))

number :: SExpr -> Either String Integer
number (SNum n) = Right n
number e = Left ("not a number: " <> show e)

-- Top level: define extends the globals, anything else is evaluated.
runForms :: Env -> [SExpr] -> [String]
runForms _ [] = []
runForms env (SList [SSym "define", SSym n, e] : rest) = case eval env e of
    Left err -> ("error: " <> err) : runForms env rest
    Right v -> (n <> " defined") : runForms (M.insert n v env) rest
runForms env (form : rest) = case eval env form of
    Left err -> ("error: " <> err) : runForms env rest
    Right v -> show v : runForms env rest

program :: String
program =
    "(define fact (lambda (n) (if (< n 2) 1 (* n (fact (- n 1))))))\n"
    <> "(fact 5) (fact 20)\n"
    <> "(define fib (lambda (n) (if (< n 2) n (+ (fib (- n 1)) (fib (- n 2))))))\n"
    <> "(fib 15)\n"
    <> "(define map (lambda (f xs) (if (null? xs) (quote ()) (cons (f (car xs)) (map f (cdr xs))))))\n"
    <> "(map (lambda (x) (* x x)) (list 1 2 3 4 5))\n"
    <> "(define compose (lambda (f g) (lambda (x) (f (g x)))))\n"
    <> "((compose (lambda (x) (+ x 1)) (lambda (x) (* x 2))) 20)\n"
    <> "(let ((a 3) (b 4)) (+ (* a a) (* b b)))\n"
    <> "(define make-adder (lambda (n) (lambda (x) (+ x n))))\n"
    <> "(define add10 (make-adder 10))\n"
    <> "(add10 5) (add10 -20)\n"
    <> "(car (quote (a b c))) (cdr (quote (a b c))) (cons 1 (quote (2 3)))\n"
    <> "(if #f 1 2) (if (quote ()) 1 2) (= 3 3) (< 4 3)\n"
    <> "(undefined-thing 1)\n"
    <> "(fact 1 2)\n"
    <> "(+ 1 (quote x))\n"
    <> "(car (quote ()))\n"
    <> "(1 2 3)\n"
    <> "(define len (lambda (xs) (if (null? xs) 0 (+ 1 (len (cdr xs))))))\n"
    <> "(len (list 1 2 3 4 5 6 7))\n"
    <> "(- 10) (- 10 1 2 3) (*)\n"
    <> "fact"

main :: IO ()
main = case readAll program of
    Left e -> putStrLn ("read error: " <> e)
    Right forms -> do
        putStrLn ("forms: " <> show (length forms))
        mapM_ putStrLn (runForms primitives forms)
        print (readAll "(1 (2")
        print (readAll ")")
        print (fmap (map show) (readAll "(a (b c) 12 -7 #t)"))
