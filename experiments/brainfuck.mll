-- Brainfuck interpreter
-- Demonstrates: algebraic data types (zipper), pattern matching on integers,
-- mutual recursion, Lua FFI for character-level string and I/O operations

import LString (strByte, strLen, strChar)

-- The tape is a zipper: cells to the left (reversed), current cell, cells to the right.
-- Cells are 8-bit unsigned integers (0–255) with wrapping arithmetic.
data Tape = Tape [Int] Int [Int]

newTape :: Tape
newTape = Tape [] 0 []

moveRight :: Tape -> Tape
moveRight (Tape ls c [])     = Tape (c:ls) 0 []
moveRight (Tape ls c (r:rs)) = Tape (c:ls) r rs

moveLeft :: Tape -> Tape
moveLeft (Tape [] c rs)     = Tape [] 0 (c:rs)
moveLeft (Tape (l:ls) c rs) = Tape ls l (c:rs)

getCell :: Tape -> Int
getCell (Tape _ c _) = c

setCell :: Int -> Tape -> Tape
setCell v (Tape ls _ rs) = Tape ls v rs

-- Find matching ']' scanning forward from pc with nesting depth
findClose :: String -> Int -> Int -> Int
findClose prog pc depth =
  if pc > strLen prog then pc
  else if strByte prog pc == 91 then findClose prog (pc + 1) (depth + 1)
  else if strByte prog pc == 93 then
    if depth == 1 then pc
    else findClose prog (pc + 1) (depth - 1)
  else findClose prog (pc + 1) depth

-- Find matching '[' scanning backward from pc with nesting depth
findOpen :: String -> Int -> Int -> Int
findOpen prog pc depth =
  if pc < 1 then pc
  else if strByte prog pc == 93 then findOpen prog (pc - 1) (depth + 1)
  else if strByte prog pc == 91 then
    if depth == 1 then pc
    else findOpen prog (pc - 1) (depth - 1)
  else findOpen prog (pc - 1) depth

-- Execute one instruction and continue
-- 62 '>'  60 '<'  43 '+'  45 '-'  46 '.'  91 '['  93 ']'
step :: Int -> String -> Int -> Tape -> IO ()
step 62 prog pc tape = run prog (pc + 1) (moveRight tape)
step 60 prog pc tape = run prog (pc + 1) (moveLeft tape)
step 43 prog pc tape = run prog (pc + 1) (setCell ((getCell tape + 1) `mod` 256) tape)
step 45 prog pc tape = run prog (pc + 1) (setCell ((getCell tape + 255) `mod` 256) tape)
step 46 prog pc tape = do
    putStr (strChar (getCell tape))
    run prog (pc + 1) tape
step 91 prog pc tape =
    if getCell tape == 0
    then run prog (findClose prog (pc + 1) 1 + 1) tape
    else run prog (pc + 1) tape
step 93 prog pc tape =
    if getCell tape /= 0
    then run prog (findOpen prog (pc - 1) 1) tape
    else run prog (pc + 1) tape
step _ prog pc tape = run prog (pc + 1) tape

-- Main interpreter loop
run :: String -> Int -> Tape -> IO ()
run prog pc tape =
    if pc > strLen prog
    then return ()
    else step (strByte prog pc) prog pc tape

-- Run a brainfuck program string
interpret :: String -> IO ()
interpret prog = run prog 1 newTape

main :: IO ()
main = do
    -- Hello World
    interpret "++++++++[>++++[>++>+++>+++>+<<<<-]>+>+>->>+[<]<-]>>.>---.+++++++..+++.>>.<-.<.+++.------.--------.>>+.>++."
    putStrLn ""

    -- Print 'A' (65 = 5 * 13)
    interpret "+++++[>+++++++++++++<-]>."
    putStrLn ""

    putStrLn "brainfuck: OK"
