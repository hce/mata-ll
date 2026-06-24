-- Recursive-descent parser: a token list for one (already de-numbered) line
-- becomes a list of statements. Expressions use precedence climbing.
--
-- Every parser threads the remaining tokens explicitly:  a PR result is
-- either an error message or (value, leftover-tokens).
module Parser (parseLine, lineNumberOf) where

import Tokens (Token(..), tokenText)
import Syntax (Expr(..), PItem(..), Stmt(..))
import Util (joinStr)
import LMath (floor)

type PR a = Either String (a, [Token])

-- Sequence two parsers, short-circuiting on the first error.
andThen :: PR a -> (a -> [Token] -> PR b) -> PR b
andThen (Left e) _ = Left e
andThen (Right (a, ts)) f = f a ts

-- ---------------------------------------------------------------------------
-- Line entry points
-- ---------------------------------------------------------------------------

-- If the token list starts with a number it is a program line number.
lineNumberOf :: [Token] -> Maybe Integer
lineNumberOf (TNum n : _) = Just (floor n)
lineNumberOf _            = Nothing

-- Parse a whole line's worth of statements (tokens AFTER any line number).
parseLine :: [Token] -> Either String [Stmt]
parseLine [] = Right []
parseLine ts =
    case parseStatements ts of
        Left e            -> Left e
        Right (sts, [])   -> Right sts
        Right (_, extra)  -> Left ("unexpected tokens: " <> showToks extra)

-- One or more statements separated by ':'.
parseStatements :: [Token] -> Either String ([Stmt], [Token])
parseStatements ts =
    parseStmt ts `andThen` \st ts1 ->
        case ts1 of
            (TOp ":" : rest) -> parseStatements rest `andThen` \sts ts2 -> Right (st : sts, ts2)
            _                -> Right ([st], ts1)

-- ---------------------------------------------------------------------------
-- Statements
-- ---------------------------------------------------------------------------

parseStmt :: [Token] -> PR Stmt
parseStmt ts =
    case ts of
        (TWord "PRINT" : rest)  -> parsePrint rest
        (TWord "LET" : rest)    -> parseAssign rest
        (TWord "IF" : rest)     -> parseIf rest
        (TWord "GOTO" : rest)   -> parseJump SGoto rest
        (TWord "GOSUB" : rest)  -> parseJump SGosub rest
        (TWord "RETURN" : rest) -> Right (SReturn, rest)
        (TWord "FOR" : rest)    -> parseFor rest
        (TWord "NEXT" : rest)   -> parseNext rest
        (TWord "INPUT" : rest)  -> parseInput rest
        (TWord "DIM" : rest)    -> parseDim rest
        (TWord "END" : rest)    -> Right (SEnd, rest)
        (TWord "STOP" : rest)   -> Right (SStop, rest)
        (TWord "REM" : _)       -> Right (SRem, [])   -- ignore rest of line
        (TWord w : rest)        -> parseImplicitLet w rest
        _                       -> Left ("expected a statement, got " <> showToks ts)

-- LET <var> = <expr>
parseAssign :: [Token] -> PR Stmt
parseAssign (TWord w : rest) = parseImplicitLet w rest
parseAssign ts = Left ("expected a variable after LET, got " <> showToks ts)

-- <var>[(<indices>)] = <expr>   (the LET keyword is optional in BASIC)
parseImplicitLet :: String -> [Token] -> PR Stmt
parseImplicitLet name ts =
    parseLValueTail name ts `andThen` \nidx ts1 ->
        expectOp "=" ts1 `andThen` \_ ts2 ->
            parseExpr ts2 `andThen` \val ts3 -> Right (SLet (fst nidx) (snd nidx) val, ts3)

-- The optional "(index, ...)" part of an assignable location.
parseLValueTail :: String -> [Token] -> PR (String, [Expr])
parseLValueTail name (TOp "(" : rest) =
    parseArgs rest `andThen` \idxs ts1 -> Right ((name, idxs), ts1)
parseLValueTail name ts = Right ((name, []), ts)

-- PRINT, with ';' and ',' separators; a trailing separator suppresses newline.
parsePrint :: [Token] -> PR Stmt
parsePrint ts = parsePrintItems ts `andThen` \items ts1 -> Right (SPrint items, ts1)

parsePrintItems :: [Token] -> PR [PItem]
parsePrintItems ts =
    case ts of
        (TOp ";" : rest)     -> parsePrintItems rest `andThen` \more ts1 -> Right (PSemi : more, ts1)
        (TOp "," : rest)     -> parsePrintItems rest `andThen` \more ts1 -> Right (PComma : more, ts1)
        (TOp ":" : _)        -> Right ([], ts)
        (TWord "ELSE" : _)   -> Right ([], ts)
        []                   -> Right ([], ts)
        _                    -> parseExpr ts `andThen` \e ts1 -> parsePrintItems ts1 `andThen` \more ts2 -> Right (PVal e : more, ts2)

-- IF <cond> THEN <then> [ELSE <else>]; a bare line number means GOTO.
parseIf :: [Token] -> PR Stmt
parseIf ts =
    parseExpr ts `andThen` \cond ts1 ->
        expectWord "THEN" ts1 `andThen` \_ ts2 ->
            parseBranch ts2 `andThen` \thenB ts3 ->
                case ts3 of
                    (TWord "ELSE" : rest) -> parseBranch rest `andThen` \elseB ts4 -> Right (SIf cond thenB elseB, ts4)
                    _                     -> Right (SIf cond thenB [], ts3)

-- A THEN/ELSE branch: a line number (implicit GOTO) or a statement sequence.
parseBranch :: [Token] -> PR [Stmt]
parseBranch (TNum n : rest) = Right ([SGoto (floor n)], rest)
parseBranch ts =
    parseStmt ts `andThen` \st ts1 ->
        case ts1 of
            (TOp ":" : rest) -> parseBranch rest `andThen` \sts ts2 -> Right (st : sts, ts2)
            _                -> Right ([st], ts1)

parseJump :: (Integer -> Stmt) -> [Token] -> PR Stmt
parseJump mk (TNum n : rest) = Right (mk (floor n), rest)
parseJump _ ts = Left ("expected a line number, got " <> showToks ts)

-- FOR <var> = <from> TO <to> [STEP <step>]
parseFor :: [Token] -> PR Stmt
parseFor (TWord v : rest) =
    expectOp "=" rest `andThen` \_ ts1 ->
        parseExpr ts1 `andThen` \from ts2 ->
            expectWord "TO" ts2 `andThen` \_ ts3 ->
                parseExpr ts3 `andThen` \to ts4 ->
                    case ts4 of
                        (TWord "STEP" : rest2) -> parseExpr rest2 `andThen` \st ts5 -> Right (SFor v from to st, ts5)
                        _                      -> Right (SFor v from to (ENum 1.0), ts4)
parseFor ts = Left ("expected a variable after FOR, got " <> showToks ts)

-- NEXT, optionally followed by a comma-separated list of loop variables.
parseNext :: [Token] -> PR Stmt
parseNext (TWord v : rest) = parseNextTail [v] rest
parseNext ts = Right (SNext [], ts)

parseNextTail :: [String] -> [Token] -> PR Stmt
parseNextTail acc (TOp "," : TWord v : rest) = parseNextTail (acc ++ [v]) rest
parseNextTail acc ts = Right (SNext acc, ts)

-- INPUT ["prompt";] var [, var ...]
parseInput :: [Token] -> PR Stmt
parseInput (TStr p : TOp ";" : rest) = parseInputVars p rest
parseInput ts = parseInputVars "" ts

parseInputVars :: String -> [Token] -> PR Stmt
parseInputVars prompt (TWord w : rest) =
    parseLValueTail w rest `andThen` \lv ts1 -> parseInputTail prompt [lv] ts1
parseInputVars _ ts = Left ("expected a variable in INPUT, got " <> showToks ts)

parseInputTail :: String -> [(String, [Expr])] -> [Token] -> PR Stmt
parseInputTail prompt acc (TOp "," : TWord w : rest) =
    parseLValueTail w rest `andThen` \lv ts1 -> parseInputTail prompt (acc ++ [lv]) ts1
parseInputTail prompt acc ts = Right (SInput prompt acc, ts)

-- DIM <name>(<sizes>) [, <name>(<sizes>) ...]
parseDim :: [Token] -> PR Stmt
parseDim ts = parseDimOne ts `andThen` \d ts1 -> parseDimTail [d] ts1

parseDimOne :: [Token] -> PR (String, [Expr])
parseDimOne (TWord w : TOp "(" : rest) =
    parseArgs rest `andThen` \dims ts1 -> Right ((w, dims), ts1)
parseDimOne ts = Left ("expected NAME(size) in DIM, got " <> showToks ts)

parseDimTail :: [(String, [Expr])] -> [Token] -> PR Stmt
parseDimTail acc (TOp "," : rest) = parseDimOne rest `andThen` \d ts1 -> parseDimTail (acc ++ [d]) ts1
parseDimTail acc ts = Right (SDim acc, ts)

-- ---------------------------------------------------------------------------
-- Expressions (precedence climbing, lowest precedence first)
-- ---------------------------------------------------------------------------

parseExpr :: [Token] -> PR Expr
parseExpr = parseOr

parseOr :: [Token] -> PR Expr
parseOr = binLevel ["OR"] parseAnd

parseAnd :: [Token] -> PR Expr
parseAnd = binLevel ["AND"] parseNot

parseNot :: [Token] -> PR Expr
parseNot (TWord "NOT" : rest) = parseNot rest `andThen` \e ts1 -> Right (EUn "NOT" e, ts1)
parseNot ts = parseCmp ts

parseCmp :: [Token] -> PR Expr
parseCmp = binLevel ["=", "<>", "<", ">", "<=", ">="] parseAdd

parseAdd :: [Token] -> PR Expr
parseAdd = binLevel ["+", "-"] parseMul

parseMul :: [Token] -> PR Expr
parseMul = binLevel ["*", "/", "MOD"] parseNeg

parseNeg :: [Token] -> PR Expr
parseNeg (TOp "-" : rest) = parseNeg rest `andThen` \e ts1 -> Right (EUn "-" e, ts1)
parseNeg ts = parsePow ts

-- '^' is right-associative.
parsePow :: [Token] -> PR Expr
parsePow ts =
    parseAtom ts `andThen` \base ts1 ->
        case ts1 of
            (TOp "^" : rest) -> parsePow rest `andThen` \ex ts2 -> Right (EBin "^" base ex, ts2)
            _                -> Right (base, ts1)

-- A left-associative level: parse a higher-precedence term, then fold in any
-- run of operators drawn from `ops` (each operator may be punctuation or a
-- keyword like AND / MOD).
binLevel :: [String] -> ([Token] -> PR Expr) -> [Token] -> PR Expr
binLevel ops sub ts = sub ts `andThen` \lhs ts1 -> binLoop ops sub lhs ts1

binLoop :: [String] -> ([Token] -> PR Expr) -> Expr -> [Token] -> PR Expr
binLoop ops sub lhs ts =
    case opHead ts of
        Just oht ->
            if elem (fst oht) ops
                then sub (snd oht) `andThen` \rhs ts2 -> binLoop ops sub (EBin (fst oht) lhs rhs) ts2
                else Right (lhs, ts)
        Nothing -> Right (lhs, ts)

-- The leading operator (punctuation or keyword) and the tokens after it.
opHead :: [Token] -> Maybe (String, [Token])
opHead (TOp o : rest)   = Just (o, rest)
opHead (TWord o : rest) = Just (o, rest)
opHead _                = Nothing

parseAtom :: [Token] -> PR Expr
parseAtom ts =
    case ts of
        (TNum n : rest)  -> Right (ENum n, rest)
        (TStr s : rest)  -> Right (EStr s, rest)
        (TOp "(" : rest) -> parseExpr rest `andThen` \e ts1 -> expectOp ")" ts1 `andThen` \_ ts2 -> Right (e, ts2)
        (TWord w : TOp "(" : rest) ->
            parseArgs rest `andThen` \args ts1 ->
                Right (if isBuiltin w then ECall w args else EArr w args, ts1)
        (TWord w : rest) -> Right (EVar w, rest)
        _                -> Left ("expected an expression, got " <> showToks ts)

-- Comma-separated expressions up to a closing ')' (the '(' is already consumed).
parseArgs :: [Token] -> PR [Expr]
parseArgs (TOp ")" : rest) = Right ([], rest)
parseArgs ts = parseExpr ts `andThen` \e ts1 -> parseArgsTail [e] ts1

parseArgsTail :: [Expr] -> [Token] -> PR [Expr]
parseArgsTail acc (TOp "," : rest) = parseExpr rest `andThen` \e ts1 -> parseArgsTail (acc ++ [e]) ts1
parseArgsTail acc (TOp ")" : rest) = Right (acc, rest)
parseArgsTail acc ts = Left ("expected ',' or ')' in argument list, got " <> showToks ts)

isBuiltin :: String -> Bool
isBuiltin w = elem w ["LEN", "LEFT$", "RIGHT$", "MID$", "CHR$", "ASC", "STR$", "VAL", "ABS", "INT", "SGN", "SQR", "RND", "SIN", "COS", "TAN", "ATN"]

-- ---------------------------------------------------------------------------
-- Small helpers
-- ---------------------------------------------------------------------------

expectOp :: String -> [Token] -> PR ()
expectOp o (TOp x : rest) = if x == o then Right ((), rest) else Left ("expected '" <> o <> "'")
expectOp o ts = Left ("expected '" <> o <> "', got " <> showToks ts)

expectWord :: String -> [Token] -> PR ()
expectWord w (TWord x : rest) = if x == w then Right ((), rest) else Left ("expected " <> w)
expectWord w ts = Left ("expected " <> w <> ", got " <> showToks ts)

showToks :: [Token] -> String
showToks ts = joinStr " " (map tokenText ts)
