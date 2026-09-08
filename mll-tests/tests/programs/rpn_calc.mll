-- A calculator: a reverse-Polish evaluator over a stack, and a
-- shunting-yard translation of infix expressions into RPN, both over a
-- token list split on spaces.
-- Leans on: Either error threading through explicit recursion, Integer
-- arithmetic including div/mod on negatives, string splitting with
-- LString, guards over String equality, list pattern matching.
import LString

splitOn :: Int -> String -> [String]
splitOn sep s = go (strToInts s)
  where
    go [] = []
    go cs = case span (/= sep) (dropWhile (== sep) cs) of
        ([], _) -> []
        (w, rest) -> mconcat (map strChar w) : go rest

isDigit :: Int -> Bool
isDigit c = c >= 48 && c <= 57

readInteger :: String -> Maybe Integer
readInteger s = case strToInts s of
    [] -> Nothing
    (45 : ds) | not (null ds) && all isDigit ds -> Just (negate (digitsValue ds))
    ds | all isDigit ds -> Just (digitsValue ds)
    _ -> Nothing
  where digitsValue = foldl (\acc d -> acc * 10 + toInteger (d - 48)) 0

evalRpn :: [String] -> Either String Integer
evalRpn = go []
  where
    go [x] [] = Right x
    go [] [] = Left "empty expression"
    go stack [] = Left ("leftover values " <> show (reverse stack))
    go stack (t : ts)
        | t == "dup" = case stack of
            (a : rest) -> go (a : a : rest) ts
            _ -> Left "dup: stack underflow"
        | t == "swap" = case stack of
            (a : b : rest) -> go (b : a : rest) ts
            _ -> Left "swap: stack underflow"
        | t == "neg" = case stack of
            (a : rest) -> go (negate a : rest) ts
            _ -> Left "neg: stack underflow"
        | isBinary t = case stack of
            (b : a : rest) -> case apply t a b of
                Left e -> Left e
                Right v -> go (v : rest) ts
            _ -> Left (t <> ": stack underflow")
        | otherwise = case readInteger t of
            Just n -> go (n : stack) ts
            Nothing -> Left ("unknown token " <> t)

isBinary :: String -> Bool
isBinary t = elem t ["+", "-", "*", "/", "%", "^"]

apply :: String -> Integer -> Integer -> Either String Integer
apply "+" a b = Right (a + b)
apply "-" a b = Right (a - b)
apply "*" a b = Right (a * b)
apply "/" a b = if b == 0 then Left "division by zero" else Right (a `div` b)
apply "%" a b = if b == 0 then Left "division by zero" else Right (a `mod` b)
apply "^" a b = if b < 0 then Left "negative exponent" else Right (a ^ fromInteger b)
apply op _ _ = Left ("unknown operator " <> op)

precedence :: String -> Int
precedence "^" = 3
precedence "*" = 2
precedence "/" = 2
precedence "%" = 2
precedence "+" = 1
precedence "-" = 1
precedence _ = 0

rightAssoc :: String -> Bool
rightAssoc op = op == "^"

-- Dijkstra's shunting yard: output queue and operator stack.
toRpn :: [String] -> Either String [String]
toRpn = go [] []
  where
    go out ops [] = flush out ops
    go out ops (t : ts)
        | t == "(" = go out (t : ops) ts
        | t == ")" = case span (/= "(") ops of
            (popped, "(" : rest) -> go (reverse popped ++ out) rest ts
            _ -> Left "mismatched )"
        | isBinary t =
            let (popped, rest) = span (\o -> o /= "(" && shouldPop t o) ops
            in go (reverse popped ++ out) (t : rest) ts
        | otherwise = case readInteger t of
            Just _ -> go (t : out) ops ts
            Nothing -> Left ("unknown token " <> t)
    flush out [] = Right (reverse out)
    flush out (o : os)
        | o == "(" = Left "mismatched ("
        | otherwise = flush (o : out) os
    shouldPop t o = precedence o > precedence t || (precedence o == precedence t && not (rightAssoc t))

joinWords :: [String] -> String
joinWords [] = ""
joinWords [w] = w
joinWords (w : ws) = w <> " " <> joinWords ws

rpnInputs :: [String]
rpnInputs =
    [ "3 4 + 2 *"
    , "5 1 2 + 4 * + 3 -"
    , "2 3 ^ 2 ^"
    , "-7 2 /"
    , "-7 2 %"
    , "7 -2 /"
    , "7 -2 %"
    , "1 dup * dup *"
    , "4 5 swap -"
    , "9 neg 2 ^"
    , "1 +"
    , "1 2 3 +"
    , "1 0 /"
    , "2 -1 ^"
    , ""
    , "1 x +"
    ]

infixInputs :: [String]
infixInputs =
    [ "1 + 2 * 3"
    , "( 1 + 2 ) * 3"
    , "2 ^ 3 ^ 2"
    , "100 / 10 / 2"
    , "100 - 10 - 2"
    , "( ( 7 ) )"
    , "2 * ( 3 + 4 ) ^ 2 - 100 % 7"
    , "( 1 + 2"
    , "1 + 2 )"
    , "1 + + 2"
    ]

main :: IO ()
main = do
    mapM_ (\src -> putStrLn (src <> " => " <> either' (evalRpn (splitOn 32 src)))) rpnInputs
    mapM_ (\src -> case toRpn (splitOn 32 src) of
        Left e -> putStrLn (src <> " => error: " <> e)
        Right rpn -> putStrLn (src <> " => " <> joinWords rpn <> " => " <> either' (evalRpn rpn))) infixInputs

either' :: Either String Integer -> String
either' (Left e) = "error: " <> e
either' (Right v) = show v
