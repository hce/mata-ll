-- Sokoban
-- A terminal-based puzzle game.  Push all boxes ($) onto goals (.).
-- Demonstrates: ADT game state, IO monad game loop, Lua FFI for raw
-- terminal input, ANSI escape rendering

import LString (strByte, strLen, strChar)
import LIO (flushStdout)
import LOS (execute)

-- Read exactly n bytes from stdin (used for single-keypress input)
readBytesRaw :: Integer -> LuaIO "io.read" String

-- Safe wrapper: returns "q" on EOF so the game exits cleanly
readKey :: IO String
readKey = do
    result <- try (readBytesRaw 1)
    case result of
        Right ch -> return ch
        Left _   -> return "q"

-- ── Types ────────────────────────────────────────────────

data Pos = Pos Integer Integer

data Game = Game [String] Pos [Pos] [Pos]
-- static map (walls/floors/goals), player, boxes, goals

data Action = Move Integer Integer | Quit | Restart | NoOp

-- ── List / position helpers ──────────────────────────────

listAt :: [a] -> Integer -> a
listAt (x:_)  1 = x
listAt (_:xs) n = listAt xs (n - 1)
listAt []     _ = error "listAt: index out of range"

posEq :: Pos -> Pos -> Bool
posEq (Pos x1 y1) (Pos x2 y2) = x1 == x2 && y1 == y2

posInList :: Pos -> [Pos] -> Bool
posInList _ []     = False
posInList p (q:qs) = if posEq p q then True else posInList p qs

hasBoxXY :: [Pos] -> Integer -> Integer -> Bool
hasBoxXY []               _ _ = False
hasBoxXY (Pos bx by : bs) x y =
    if bx == x && by == y then True else hasBoxXY bs x y

-- ── Map queries ──────────────────────────────────────────

isWall :: [String] -> Integer -> Integer -> Bool
isWall smap x y =
    if y < 1 || y > length smap then True
    else let row = listAt smap y
         in if x < 1 || x > strLen row then True
            else strByte row x == 35

-- ── Movement ─────────────────────────────────────────────

moveBox :: [Pos] -> Integer -> Integer -> Integer -> Integer -> [Pos]
moveBox []               _  _  _  _  = []
moveBox (Pos bx by : bs) fx fy tx ty =
    if bx == fx && by == fy
    then Pos tx ty : bs
    else Pos bx by : moveBox bs fx fy tx ty

tryMove :: Game -> Integer -> Integer -> Game
tryMove (Game smap (Pos px py) boxes goals) dx dy =
    let tx = px + dx
        ty = py + dy
        bx = tx + dx
        by = ty + dy
    in if isWall smap tx ty
       then Game smap (Pos px py) boxes goals
       else if hasBoxXY boxes tx ty
            then if isWall smap bx by || hasBoxXY boxes bx by
                 then Game smap (Pos px py) boxes goals
                 else Game smap (Pos tx ty) (moveBox boxes tx ty bx by) goals
            else Game smap (Pos tx ty) boxes goals

-- ── Win check ────────────────────────────────────────────

isWin :: [Pos] -> [Pos] -> Bool
isWin []     _     = True
isWin (g:gs) boxes = if posInList g boxes then isWin gs boxes else False

-- ── Input ────────────────────────────────────────────────

charToAction :: Integer -> Action
charToAction 119 = Move 0 (-1)      -- w
charToAction 107 = Move 0 (-1)      -- k
charToAction 115 = Move 0 1         -- s
charToAction 106 = Move 0 1         -- j
charToAction  97 = Move (-1) 0      -- a
charToAction 104 = Move (-1) 0      -- h
charToAction 100 = Move 1 0         -- d
charToAction 108 = Move 1 0         -- l
charToAction 113 = Quit             -- q
charToAction   3 = Quit             -- ctrl-c
charToAction 114 = Restart          -- r
charToAction   _ = NoOp

-- ── Rendering ────────────────────────────────────────────

esc :: String
esc = strChar 27

clearScreen :: IO ()
clearScreen = putStr (esc <> "[2J" <> esc <> "[H")

cellChar :: String -> Integer -> Pos -> [Pos] -> Integer -> String
cellChar row y (Pos px py) boxes x =
    if px == x && py == y
    then if strByte row x == 46 then "+" else "@"
    else if hasBoxXY boxes x y
    then if strByte row x == 46 then "*" else "$"
    else strChar (strByte row x)

buildRow :: String -> Integer -> Pos -> [Pos] -> Integer -> String
buildRow row y player boxes x =
    if x > strLen row then ""
    else cellChar row y player boxes x <> buildRow row y player boxes (x + 1)

renderRows :: [String] -> Pos -> [Pos] -> Integer -> IO ()
renderRows []         _      _     _ = return ()
renderRows (row:rest) player boxes y = do
    putStrLn (buildRow row y player boxes 1)
    renderRows rest player boxes (y + 1)

render :: Game -> Integer -> Integer -> IO ()
render (Game smap player boxes _) lvl moves = do
    clearScreen
    putStrLn ("Level " <> show lvl <> "   Moves: " <> show moves)
    putStrLn ""
    renderRows smap player boxes 1
    putStrLn ""
    putStrLn "wasd/hjkl:move  r:restart  q:quit"
    flushStdout

-- ── Level parsing ────────────────────────────────────────

-- Strip player/boxes from a cell, leaving the underlying floor or goal
staticChar :: Integer -> Integer
staticChar 64 = 32     -- @ -> space
staticChar 36 = 32     -- $ -> space
staticChar 43 = 46     -- + -> .
staticChar 42 = 46     -- * -> .
staticChar c  = c

buildStaticRow :: String -> Integer -> String
buildStaticRow s i =
    if i > strLen s then ""
    else strChar (staticChar (strByte s i)) <> buildStaticRow s (i + 1)

buildStaticMap :: [String] -> [String]
buildStaticMap []     = []
buildStaticMap (r:rs) = buildStaticRow r 1 : buildStaticMap rs

findPlayerRow :: String -> Integer -> Maybe Integer
findPlayerRow s x =
    if x > strLen s then Nothing
    else let ch = strByte s x
         in if ch == 64 || ch == 43 then Just x    -- @ or +
            else findPlayerRow s (x + 1)

findPlayer :: [String] -> Integer -> Pos
findPlayer []         _ = Pos 1 1
findPlayer (row:rest) y =
    case findPlayerRow row 1 of
        Just x  -> Pos x y
        Nothing -> findPlayer rest (y + 1)

findBoxesRow :: String -> Integer -> Integer -> [Pos]
findBoxesRow s y x =
    if x > strLen s then []
    else let ch  = strByte s x
             more = findBoxesRow s y (x + 1)
         in if ch == 36 || ch == 42 then Pos x y : more else more

findBoxes :: [String] -> Integer -> [Pos]
findBoxes []         _ = []
findBoxes (row:rest) y = findBoxesRow row y 1 ++ findBoxes rest (y + 1)

findGoalsRow :: String -> Integer -> Integer -> [Pos]
findGoalsRow s y x =
    if x > strLen s then []
    else let ch   = strByte s x
             more = findGoalsRow s y (x + 1)
         in if ch == 46 || ch == 42 || ch == 43   -- . * +
            then Pos x y : more else more

findGoals :: [String] -> Integer -> [Pos]
findGoals []         _ = []
findGoals (row:rest) y = findGoalsRow row y 1 ++ findGoals rest (y + 1)

parseLevel :: [String] -> Game
parseLevel rows = Game (buildStaticMap rows)
                       (findPlayer rows 1)
                       (findBoxes rows 1)
                       (findGoals rows 1)

-- ── Game loop ────────────────────────────────────────────

playLevel :: Game -> Game -> Integer -> Integer -> IO Bool
playLevel initial current lvl moves = do
    render current lvl moves
    if isWin (gameGoals current) (gameBoxes current)
    then do
        putStr (esc <> "[20;1H")
        putStrLn "  Level complete!  Press any key..."
        flushStdout
        _ <- readKey
        return True
    else do
        ch <- readKey
        case charToAction (strByte ch 1) of
            Quit    -> return False
            Restart -> playLevel initial initial lvl 0
            NoOp   -> playLevel initial current lvl moves
            Move dx dy ->
                playLevel initial (tryMove current dx dy) lvl (moves + 1)

gameGoals :: Game -> [Pos]
gameGoals (Game _ _ _ gs) = gs

gameBoxes :: Game -> [Pos]
gameBoxes (Game _ _ bs _) = bs

playLevels :: [Game] -> Integer -> IO ()
playLevels []     _ = do
    clearScreen
    putStrLn "Congratulations!  All levels complete!"
    putStrLn ""
playLevels (g:gs) n = do
    won <- playLevel g g n 0
    if won then playLevels gs (n + 1)
    else return ()

-- ── Levels ───────────────────────────────────────────────

level1 :: [String]
level1 = [ "#####"
         , "#. @#"
         , "# $ #"
         , "#   #"
         , "#####"
         ]

level2 :: [String]
level2 = [ "######"
         , "#.   #"
         , "#  $ #"
         , "#  $ #"
         , "# .@ #"
         , "######"
         ]

level3 :: [String]
level3 = [ "########"
         , "#  . . #"
         , "# $ $  #"
         , "#   @  #"
         , "# $    #"
         , "#  .   #"
         , "########"
         ]

-- ── Main ─────────────────────────────────────────────────

main :: IO ()
main = do
    _ <- execute "stty -icanon -echo"
    let games = [ parseLevel level1
                , parseLevel level2
                , parseLevel level3
                ]
    playLevels games 1
    _ <- execute "stty sane"
    return ()
