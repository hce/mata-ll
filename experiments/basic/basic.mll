-- mata-ll BASIC: a small Microsoft-style BASIC interpreter.
--
--   mll -r basic.mll program.bas     -- load and run a .bas file
--   mll -r basic.mll                 -- start the interactive REPL
--
-- In the REPL, a line that starts with a number is stored in the program;
-- RUN executes it, LIST prints it, NEW clears it, BYE quits. A line with no
-- number is executed immediately.

import Lexer (tokenize)
import Parser (parseLine, lineNumberOf)
import Tokens (Token(..))
import Syntax (Stmt)
import Interp (State, buildState, run, execImmediate)
import LIO (fileLines, readLine)
import Data.List (sortBy)

main :: IO ()
main = do
    args <- getArgs
    case args of
        []         -> startRepl
        (path : _) -> loadAndRun path

-- ---------------------------------------------------------------------------
-- File mode
-- ---------------------------------------------------------------------------

loadAndRun :: String -> IO ()
loadAndRun path = do
    ls <- fileLines path
    run (buildState (sortProg (parseSource ls)))

-- Parse every numbered, non-blank source line into (lineNo, statements).
parseSource :: [String] -> [(Int, [Stmt])]
parseSource [] = []
parseSource (line : rest) =
    let toks = tokenize line
    in case lineNumberOf toks of
        Nothing -> parseSource rest          -- blank or unnumbered: skip
        Just n  -> case parseLine (tail toks) of
            Left e   -> error ("line " <> show n <> ": " <> e)
            Right ss -> (n, ss) : parseSource rest

sortProg :: [(Int, [Stmt])] -> [(Int, [Stmt])]
sortProg = sortBy (\a b -> compare (fst a) (fst b))

-- ---------------------------------------------------------------------------
-- Interactive REPL
-- ---------------------------------------------------------------------------

startRepl :: IO ()
startRepl = do
    putStrLn "mata-ll BASIC -- numbered lines build a program; RUN, LIST, NEW, BYE."
    replLoop hmEmpty

-- The program under construction maps a line number to (source text,
-- statements): the text is kept for LIST, the statements for RUN.
replLoop :: HashMap Int (String, [Stmt]) -> IO ()
replLoop prog = do
    putStr "] "
    line <- readLine
    let toks = tokenize line
    case toks of
        [] -> replLoop prog
        (TNum n : rest) ->
            if null rest
                then replLoop (hmDelete (lineNo n) prog)          -- "10" alone deletes
                else case parseLine rest of
                    Left e   -> putStrLn ("?SYNTAX: " <> e) >> replLoop prog
                    Right ss -> replLoop (hmInsert (lineNo n) (line, ss) prog)
        (TWord "RUN" : _)  -> run (buildState (programOf prog)) >> replLoop prog
        (TWord "LIST" : _) -> listProg prog >> replLoop prog
        (TWord "NEW" : _)  -> replLoop hmEmpty
        (TWord "BYE" : _)  -> putStrLn "bye"
        (TWord "QUIT" : _) -> putStrLn "bye"
        _ -> case parseLine toks of
            Left e   -> putStrLn ("?SYNTAX: " <> e) >> replLoop prog
            Right ss -> execImmediate ss >> replLoop prog

lineNo :: Number -> Int
lineNo = floorLine

floorLine :: Number -> LuaPure "math.floor" Int

-- The runnable program: statements in line-number order.
programOf :: HashMap Int (String, [Stmt]) -> [(Int, [Stmt])]
programOf prog =
    map (\k -> (k, snd (entryAt k prog))) (sortBy (\a b -> compare a b) (hmKeys prog))

listProg :: HashMap Int (String, [Stmt]) -> IO ()
listProg prog = mapM_ (\k -> putStrLn (fst (entryAt k prog))) (sortBy (\a b -> compare a b) (hmKeys prog))

entryAt :: Int -> HashMap Int (String, [Stmt]) -> (String, [Stmt])
entryAt k prog = case hmLookup k prog of
    Just e  -> e
    Nothing -> ("", [])
