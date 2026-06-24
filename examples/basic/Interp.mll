-- The interpreter proper: program state, expression evaluation, and statement
-- execution. Output (PRINT) and input (INPUT) make execution live in IO; the
-- rest of the state is an immutable record threaded through the run loop.
module Interp (State, buildState, run, execImmediate) where

import Syntax (Expr(..), PItem(..), Stmt(..))
import Value (Value(..), showVal, asNum, asStr, isTrue, fromBool)
import Util (concatStr)
import Data.Maybe (fromMaybe)
import LString (strByte, strLen, strSub, strChar)
import LMath (floor, abs, sqrt, sin, cos, tan, atan, fmod, exp, log)
import LIO (readLine)

-- FFI bindings that return Number directly, so counts and truncations don't
-- have to bounce through Integer (which mata-ll keeps distinct from Number).
lenNum :: String -> LuaPure "string.len" Number
byteNum :: String -> Integer -> LuaPure "string.byte" Number
floorNum :: Number -> LuaPure "math.floor" Number

-- A dimensioned array: the per-dimension upper bounds and a sparse store of
-- element values keyed by row-major flat index.
data BArr = BArr [Integer] (HashMap Integer Value)

-- An active FOR loop: control variable, limit, step, and the flat index of the
-- statement just after FOR (where NEXT jumps back to).
data ForFrame = ForFrame String Number Number Integer

-- The whole machine. stCode maps a flat statement index to its statement;
-- stLineIx maps a BASIC line number to the flat index it starts at.
data State = State { stVars :: HashMap String Value, stArrays :: HashMap String BArr, stCode :: HashMap Integer Stmt, stLineIx :: HashMap Integer Integer, stCount :: Integer, stPC :: Integer, stSubs :: [Integer], stFors :: [ForFrame] }

-- What to do with the program counter after a statement runs.
data Flow = FNext | FJump Integer | FHalt

-- ---------------------------------------------------------------------------
-- Building the initial state from parsed, line-number-sorted source
-- ---------------------------------------------------------------------------

buildState :: [(Integer, [Stmt])] -> State
buildState prog =
    let cells = concat (map (\lns -> map (\s -> (fst lns, s)) (snd lns)) prog)
        code = buildCode 0 cells
        lineix = buildLineIx 0 cells
    in State hmEmpty hmEmpty code lineix (length cells) 0 [] []

buildCode :: Integer -> [(Integer, Stmt)] -> HashMap Integer Stmt
buildCode _ [] = hmEmpty
buildCode i (c : rest) = hmInsert i (snd c) (buildCode (i + 1) rest)

-- Map each line number to its first flat index. Recursing before inserting
-- means the smallest (earliest) index for a line wins.
buildLineIx :: Integer -> [(Integer, Stmt)] -> HashMap Integer Integer
buildLineIx _ [] = hmEmpty
buildLineIx i (c : rest) = hmInsert (fst c) i (buildLineIx (i + 1) rest)

-- ---------------------------------------------------------------------------
-- The run loop
-- ---------------------------------------------------------------------------

run :: State -> IO ()
run st =
    if stPC st >= stCount st then return ()
    else case hmLookup (stPC st) (stCode st) of
        Nothing -> return ()
        Just stmt -> do
            res <- exec st stmt
            case res of
                (st1, FNext)   -> run (st1 { stPC = stPC st1 + 1 })
                (st1, FJump t) -> run (st1 { stPC = t })
                (st1, FHalt)   -> return ()

-- Execute statements typed at the REPL with no line number, in a throwaway
-- state (variables do not persist between immediate lines).
execImmediate :: [Stmt] -> IO ()
execImmediate sts = do
    _ <- execSeq (buildState []) sts
    return ()

-- Run a sequence of statements (a THEN/ELSE branch). Stop early if one of them
-- transfers control.
execSeq :: State -> [Stmt] -> IO (State, Flow)
execSeq st [] = return (st, FNext)
execSeq st (s : rest) = do
    res <- exec st s
    case res of
        (st1, FNext) -> execSeq st1 rest
        other        -> return other

-- ---------------------------------------------------------------------------
-- Executing one statement
-- ---------------------------------------------------------------------------

exec :: State -> Stmt -> IO (State, Flow)
exec st SRem = return (st, FNext)
exec st SEnd = return (st, FHalt)
exec st SStop = return (st, FHalt)
exec st (SGoto ln) = return (st, FJump (resolveLine st ln))
exec st (SGosub ln) = return (st { stSubs = (stPC st + 1) : stSubs st }, FJump (resolveLine st ln))
exec st SReturn =
    case stSubs st of
        []          -> error "RETURN without GOSUB"
        (r : rest)  -> return (st { stSubs = rest }, FJump r)
exec st (SLet name idxs valE) =
    let v = eval st valE
    in return (assign st name idxs v, FNext)
exec st (SIf cond thenB elseB) =
    if isTrue (eval st cond) then execSeq st thenB else execSeq st elseB
exec st (SDim decls) = return (foldl dimOne st decls, FNext)
exec st (SPrint items) = do
    let txt = fst (renderPrint st 0.0 items)
    if endsOpen items then putStr txt else putStrLn txt
    return (st, FNext)
exec st (SFor var fromE toE stepE) =
    let fromV = asNum (eval st fromE)
        toV   = asNum (eval st toE)
        stepV = asNum (eval st stepE)
        st1   = setScalar st var (VNum fromV)
        frame = ForFrame var toV stepV (stPC st + 1)
    in return (st1 { stFors = frame : stFors st1 }, FNext)
exec st (SNext vars) = return (doNext st vars)
exec st (SInput prompt targets) = do
    putStr (if prompt == "" then "? " else prompt <> "? ")
    line <- readLine
    let st1 = assignInputs st targets (splitComma line)
    return (st1, FNext)

resolveLine :: State -> Integer -> Integer
resolveLine st ln = fromMaybe (error ("undefined line " <> show ln)) (hmLookup ln (stLineIx st))

-- FOR/NEXT: advance the innermost matching loop; jump back if it should run
-- again, otherwise pop it and fall through.
doNext :: State -> [String] -> (State, Flow)
doNext st _ =
    case stFors st of
        [] -> error "NEXT without FOR"
        (ForFrame var limit step back : rest) ->
            let cur  = asNum (lookupScalar st var)
                next = cur + step
                st1  = setScalar st var (VNum next)
                cont = if step >= 0.0 then next <= limit else next >= limit
            in if cont then (st1, FJump back) else (st1 { stFors = rest }, FNext)

-- ---------------------------------------------------------------------------
-- Assignment
-- ---------------------------------------------------------------------------

assign :: State -> String -> [Expr] -> Value -> State
assign st name [] v = setScalar st name v
assign st name idxs v = writeArr st name (map (\e -> asNum (eval st e)) idxs) v

setScalar :: State -> String -> Value -> State
setScalar st name v = st { stVars = hmInsert name v (stVars st) }

lookupScalar :: State -> String -> Value
lookupScalar st name = fromMaybe (defaultFor name) (hmLookup name (stVars st))

defaultFor :: String -> Value
defaultFor name = if isStrName name then VStr "" else VNum 0.0

isStrName :: String -> Bool
isStrName name = strByte name (strLen name) == 36   -- trailing '$'

dimOne :: State -> (String, [Expr]) -> State
dimOne st decl =
    let dims = map (\e -> floor (asNum (eval st e))) (snd decl)
    in st { stArrays = hmInsert (fst decl) (BArr dims hmEmpty) (stArrays st) }

-- Look the array up (auto-dimensioning a missing one to bound 10 per index),
-- then read or write the row-major element.
getArr :: State -> String -> [Integer] -> BArr
getArr st name idxs = fromMaybe (BArr (map (\_ -> 10) idxs) hmEmpty) (hmLookup name (stArrays st))

flatIndex :: [Integer] -> [Integer] -> Integer
flatIndex dims idxs = foldl (\acc di -> acc * (fst di + 1) + snd di) 0 (zip dims idxs)

readArr :: State -> String -> [Number] -> Value
readArr st name idxsN =
    let idxs = map floor idxsN
    in case getArr st name idxs of
        BArr dims dat -> fromMaybe (defaultFor name) (hmLookup (flatIndex dims idxs) dat)

writeArr :: State -> String -> [Number] -> Value -> State
writeArr st name idxsN v =
    let idxs = map floor idxsN
    in case getArr st name idxs of
        BArr dims dat ->
            let dat1 = hmInsert (flatIndex dims idxs) v dat
            in st { stArrays = hmInsert name (BArr dims dat1) (stArrays st) }

-- ---------------------------------------------------------------------------
-- Expression evaluation (pure)
-- ---------------------------------------------------------------------------

eval :: State -> Expr -> Value
eval _ (ENum n) = VNum n
eval _ (EStr s) = VStr s
eval st (EVar name) = lookupScalar st name
eval st (EArr name idxs) = readArr st name (map (\e -> asNum (eval st e)) idxs)
eval st (ECall fn args) = callBuiltin fn (map (eval st) args)
eval st (EUn "-" e) = VNum (0.0 - asNum (eval st e))
eval st (EUn "NOT" e) = fromBool (not (isTrue (eval st e)))
eval _ (EUn op _) = error ("unknown unary operator " <> op)
eval st (EBin op l r) = evalBin op (eval st l) (eval st r)

evalBin :: String -> Value -> Value -> Value
evalBin "+" (VStr a) (VStr b) = VStr (a <> b)
evalBin "+" a b = VNum (asNum a + asNum b)
evalBin "-" a b = VNum (asNum a - asNum b)
evalBin "*" a b = VNum (asNum a * asNum b)
evalBin "/" a b = VNum (asNum a / asNum b)
evalBin "^" a b = VNum (powNum (asNum a) (asNum b))
evalBin "MOD" a b = VNum (fmod (asNum a) (asNum b))
evalBin "AND" a b = fromBool (isTrue a && isTrue b)
evalBin "OR" a b = fromBool (isTrue a || isTrue b)
evalBin op a b = fromBool (compareOp op a b)

compareOp :: String -> Value -> Value -> Bool
compareOp "=" a b = valEq a b
compareOp "<>" a b = not (valEq a b)
compareOp "<" a b = valLt a b
compareOp ">" a b = valLt b a
compareOp "<=" a b = not (valLt b a)
compareOp ">=" a b = not (valLt a b)
compareOp op _ _ = error ("unknown operator " <> op)

valEq :: Value -> Value -> Bool
valEq (VNum a) (VNum b) = a == b
valEq (VStr a) (VStr b) = a == b
valEq _ _ = error "type mismatch in comparison"

valLt :: Value -> Value -> Bool
valLt (VNum a) (VNum b) = a < b
valLt (VStr a) (VStr b) = a < b
valLt _ _ = error "type mismatch in comparison"

-- ---------------------------------------------------------------------------
-- Built-in functions
-- ---------------------------------------------------------------------------

callBuiltin :: String -> [Value] -> Value
callBuiltin "LEN" [s] = VNum (lenNum (asStr s))
callBuiltin "LEFT$" [s, n] = VStr (strSub (asStr s) 1 (floor (asNum n)))
callBuiltin "RIGHT$" [s, n] =
    let str = asStr s
        len = strLen str
        k = floor (asNum n)
    in VStr (strSub str (len - k + 1) len)
callBuiltin "MID$" [s, i, n] =
    let start = floor (asNum i)
    in VStr (strSub (asStr s) start (start + floor (asNum n) - 1))
callBuiltin "CHR$" [n] = VStr (strChar (floor (asNum n)))
callBuiltin "ASC" [s] = VNum (byteNum (asStr s) 1)
callBuiltin "STR$" [n] = VStr (showVal (VNum (asNum n)))
callBuiltin "VAL" [s] = VNum (read_Number (asStr s))
callBuiltin "ABS" [n] = VNum (abs (asNum n))
callBuiltin "INT" [n] = VNum (floorNum (asNum n))
callBuiltin "SGN" [n] = VNum (sgn (asNum n))
callBuiltin "SQR" [n] = VNum (sqrt (asNum n))
callBuiltin "SIN" [n] = VNum (sin (asNum n))
callBuiltin "COS" [n] = VNum (cos (asNum n))
callBuiltin "TAN" [n] = VNum (tan (asNum n))
callBuiltin "ATN" [n] = VNum (atan (asNum n))
-- RND would need IO (math.random is effectful), but expressions evaluate
-- purely here, so it cannot be supported without reworking evaluation.
callBuiltin "RND" _ = error "RND is not supported: expression evaluation is pure, so it cannot produce randomness"
callBuiltin fn _ = error ("unknown function " <> fn)

sgn :: Number -> Number
sgn n = if n > 0.0 then 1.0 else if n < 0.0 then 0.0 - 1.0 else 0.0

-- mata-ll has no value-level '^', so exponentiation is done here: exact
-- repeated multiplication for integer exponents (the common BASIC case, and
-- avoids float error on e.g. 2^10), and exp/log for fractional ones.
powNum :: Number -> Number -> Number
powNum base ex =
    if fmod ex 1.0 == 0.0 then intPow base (floor ex) else exp (ex * log base)

intPow :: Number -> Integer -> Number
intPow base e =
    if e == 0 then 1.0
    else if e < 0 then 1.0 / intPow base (0 - e)
    else base * intPow base (e - 1)

-- ---------------------------------------------------------------------------
-- PRINT rendering
-- ---------------------------------------------------------------------------

-- A trailing ';' or ',' leaves the line open (no newline).
endsOpen :: [PItem] -> Bool
endsOpen [] = False
endsOpen [PSemi] = True
endsOpen [PComma] = True
endsOpen (_ : rest) = endsOpen rest

-- Build the output text, tracking the current column so ',' can tab to the
-- next 14-character zone. Returns (text, column).
renderPrint :: State -> Number -> [PItem] -> (String, Number)
renderPrint _ col [] = ("", col)
renderPrint st col (PSemi : rest) = renderPrint st col rest
renderPrint st col (PComma : rest) =
    let pad = zonePad col
        more = renderPrint st (col + lenNum pad) rest
    in (pad <> fst more, snd more)
renderPrint st col (PVal e : rest) =
    let s = showVal (eval st e)
        more = renderPrint st (col + lenNum s) rest
    in (s <> fst more, snd more)

zonePad :: Number -> String
zonePad col = spaces (floor (14.0 - fmod col 14.0))

spaces :: Integer -> String
spaces k = if k <= 0 then "" else " " <> spaces (k - 1)

-- ---------------------------------------------------------------------------
-- INPUT
-- ---------------------------------------------------------------------------

assignInputs :: State -> [(String, [Expr])] -> [String] -> State
assignInputs st [] _ = st
assignInputs st _ [] = st
assignInputs st (t : ts) (v : vs) =
    let value = if isStrName (fst t) then VStr v else VNum (read_Number v)
        st1 = assign st (fst t) (snd t) value
    in assignInputs st1 ts vs

-- Split a line on commas (no quoting -- adequate for INPUT).
splitComma :: String -> [String]
splitComma s = go 1 1
  where
    n = strLen s
    go start i =
        if i > n then [strSub s start n]
        else if strByte s i == 44 then strSub s start (i - 1) : go (i + 1) (i + 1)
        else go start (i + 1)
