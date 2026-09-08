-- A Brainfuck interpreter: the tape is a zipper of Int cells, the
-- program a list of instruction codes with bracket targets precomputed
-- into a map, and the output accumulated as a String.
-- Leans on: an explicit machine-state record stepped in a tail-recursive
-- loop, Data.Map for jump tables, Int wrap-around arithmetic by mod,
-- LString for program text and output characters, a step budget.
import LString
import Data.Map (Map)
import qualified Data.Map as M

data Tape = Tape [Int] Int [Int]

data Machine = Machine
    { tape :: Tape
    , pc :: Int
    , output :: [Int]
    , steps :: Int
    }

-- Brackets matched with a stack, both directions recorded.
bracketMap :: [Int] -> Either String (Map Int Int)
bracketMap prog = go 0 [] M.empty prog
  where
    go _ [] m [] = Right m
    go _ _ _ [] = Left "unmatched ["
    go i stack m (c : cs)
        | c == 91 = go (i + 1) (i : stack) m cs
        | c == 93 = case stack of
            [] -> Left "unmatched ]"
            (open : rest) -> go (i + 1) rest (M.insert open i (M.insert i open m)) cs
        | otherwise = go (i + 1) stack m cs

moveLeft :: Tape -> Tape
moveLeft (Tape [] c rs) = Tape [] 0 (c : rs)
moveLeft (Tape (l : ls) c rs) = Tape ls l (c : rs)

moveRight :: Tape -> Tape
moveRight (Tape ls c []) = Tape (c : ls) 0 []
moveRight (Tape ls c (r : rs)) = Tape (c : ls) r rs

cell :: Tape -> Int
cell (Tape _ c _) = c

setCell :: Int -> Tape -> Tape
setCell v (Tape ls _ rs) = Tape ls (v `mod` 256) rs

run :: Int -> [Int] -> [Int] -> Either String (String, Int)
run budget prog input = case bracketMap prog of
    Left e -> Left e
    Right jumps -> loop jumps input (Machine { tape = Tape [] 0 [], pc = 0, output = [], steps = 0 })
  where
    len = length prog
    loop jumps inp m
        | steps m >= budget = Left ("step budget exhausted at pc " <> show (pc m))
        | pc m >= len = Right (mconcat (map strChar (reverse (output m))), steps m)
        | otherwise =
            let op = prog !! pc m
                t = tape m
                next = m { pc = pc m + 1, steps = steps m + 1 }
            in if op == 62 then loop jumps inp next { tape = moveRight t }
               else if op == 60 then loop jumps inp next { tape = moveLeft t }
               else if op == 43 then loop jumps inp next { tape = setCell (cell t + 1) t }
               else if op == 45 then loop jumps inp next { tape = setCell (cell t - 1) t }
               else if op == 46 then loop jumps inp next { output = cell t : output m }
               else if op == 44 then case inp of
                   [] -> loop jumps [] next { tape = setCell 0 t }
                   (i : rest) -> loop jumps rest next { tape = setCell i t }
               else if op == 91 && cell t == 0 then loop jumps inp next { pc = M.findWithDefault len (pc m) jumps + 1 }
               else if op == 93 && cell t /= 0 then loop jumps inp next { pc = M.findWithDefault len (pc m) jumps + 1 }
               else loop jumps inp next

helloWorld :: String
helloWorld = "++++++++[>++++[>++>+++>+++>+<<<<-]>+>+>->>+[<]<-]>>.>---.+++++++..+++.>>.<-.<.+++.------.--------.>>+.>++."

-- Reads two digits and prints their sum as a digit.
adder :: String
adder = ",>,[<+>-]<------------------------------------------------."

-- Echo input until a zero byte, uppercasing letters.
upper :: String
upper = ",[>++++++++[<---->-]<.,]"

reverser :: String
reverser = ">,[>,]<[.<]"

report :: String -> String -> String -> IO ()
report label prog input = do
    putStrLn (label <> ":")
    case run 100000 (strToInts prog) (strToInts input) of
        Left e -> putStrLn ("  error: " <> e)
        Right (out, n) -> do
            putStrLn ("  output: " <> show out)
            putStrLn ("  steps: " <> show n)

main :: IO ()
main = do
    report "hello" helloWorld ""
    report "adder" adder "34"
    report "adder" adder "99"
    report "upper" upper "hello, World"
    report "reverser" reverser "stressed"
    report "unmatched" "+[>+" ""
    report "unmatched" "+]" ""
    report "infinite" "+[]" ""
    report "wrap" "--+++." ""
    report "empty" "" "x"
    print (fmap M.toList (bracketMap (strToInts "[[][]]")))
