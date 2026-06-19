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

data Pos = Pos { posCol :: Integer, posRow :: Integer }
    deriving (Eq)

data Game = Game [String] Pos [Pos] [Pos]
-- static map (walls/floors/goals), player, boxes, goals

gameBoxes :: Game -> [Pos]
gameBoxes (Game _ _ bs _) = bs

gameGoals :: Game -> [Pos]
gameGoals (Game _ _ _ gs) = gs

data Action = Move Integer Integer | Quit | Restart | NoOp

-- ── Sokoban tile codes ───────────────────────────────────

chWall :: Integer
chWall = 35            -- '#'

chGoal :: Integer
chGoal = 46            -- '.'

chPlayer :: Integer
chPlayer = 64          -- '@'

chBox :: Integer
chBox = 36             -- '$'

chBoxOnGoal :: Integer
chBoxOnGoal = 42       -- '*'

chPlayerOnGoal :: Integer
chPlayerOnGoal = 43    -- '+'

-- ── Helpers ──────────────────────────────────────────────

listAt :: [a] -> Integer -> a
listAt (x:_)  1 = x
listAt (_:xs) n = listAt xs (n - 1)
listAt []     _ = error "listAt: index out of range"

hasBoxAt :: [Pos] -> Integer -> Integer -> Bool
hasBoxAt boxes x y = elem (Pos x y) boxes

-- ── Map queries ──────────────────────────────────────────

isWall :: [String] -> Integer -> Integer -> Bool
isWall smap x y
    | y < 1 || y > length smap = True
    | x < 1 || x > strLen row  = True
    | otherwise                 = strByte row x == chWall
  where row = listAt smap y

-- ── Movement ─────────────────────────────────────────────

moveBox :: [Pos] -> Pos -> Pos -> [Pos]
moveBox []     _    _  = []
moveBox (b:bs) from to =
    if b == from then to : bs
    else b : moveBox bs from to

tryMove :: Game -> Integer -> Integer -> Game
tryMove (Game smap (Pos px py) boxes goals) dx dy =
    let tx     = px + dx
        ty     = py + dy
        bx     = tx + dx
        by     = ty + dy
        stay   = Game smap (Pos px py) boxes goals
        target = Pos tx ty
        beyond = Pos bx by
    in if isWall smap tx ty then stay
       else if hasBoxAt boxes tx ty
            then if isWall smap bx by || hasBoxAt boxes bx by
                 then stay
                 else Game smap target (moveBox boxes target beyond) goals
            else Game smap target boxes goals

-- ── Win check ────────────────────────────────────────────

isWin :: [Pos] -> [Pos] -> Bool
isWin []     _     = True
isWin (g:gs) boxes = elem g boxes && isWin gs boxes

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
cellChar row y (Pos px py) boxes x
    | px == x && py == y = if onGoal then "+" else "@"
    | hasBoxAt boxes x y = if onGoal then "*" else "$"
    | otherwise          = strChar (strByte row x)
  where onGoal = strByte row x == chGoal

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
staticChar c
    | c == chPlayer || c == chBox            = 32   -- floor
    | c == chPlayerOnGoal || c == chBoxOnGoal = chGoal
    | otherwise                              = c

buildStaticRow :: String -> Integer -> String
buildStaticRow s i =
    if i > strLen s then ""
    else strChar (staticChar (strByte s i)) <> buildStaticRow s (i + 1)

buildStaticMap :: [String] -> [String]
buildStaticMap []     = []
buildStaticMap (r:rs) = buildStaticRow r 1 : buildStaticMap rs

-- Scan a row for positions whose character code satisfies a predicate
findInRow :: String -> Integer -> Integer -> (Integer -> Bool) -> [Pos]
findInRow s y x match =
    if x > strLen s then []
    else let rest = findInRow s y (x + 1) match
         in if match (strByte s x) then Pos x y : rest else rest

findAll :: [String] -> Integer -> (Integer -> Bool) -> [Pos]
findAll []         _ _     = []
findAll (row:rest) y match = findInRow row y 1 match ++ findAll rest (y + 1) match

isBoxChar :: Integer -> Bool
isBoxChar c = c == chBox || c == chBoxOnGoal

isGoalChar :: Integer -> Bool
isGoalChar c = c == chGoal || c == chBoxOnGoal || c == chPlayerOnGoal

findPlayerRow :: String -> Integer -> Maybe Integer
findPlayerRow s x
    | x > strLen s                          = Nothing
    | ch == chPlayer || ch == chPlayerOnGoal = Just x
    | otherwise                             = findPlayerRow s (x + 1)
  where ch = strByte s x

findPlayer :: [String] -> Integer -> Pos
findPlayer []         _ = Pos 1 1
findPlayer (row:rest) y =
    case findPlayerRow row 1 of
        Just x  -> Pos x y
        Nothing -> findPlayer rest (y + 1)

parseLevel :: [String] -> Game
parseLevel rows = Game (buildStaticMap rows)
                       (findPlayer rows 1)
                       (findAll rows 1 isBoxChar)
                       (findAll rows 1 isGoalChar)

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
            Quit       -> return False
            Restart    -> playLevel initial initial lvl 0
            NoOp       -> playLevel initial current lvl moves
            Move dx dy -> playLevel initial (tryMove current dx dy) lvl (moves + 1)

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
