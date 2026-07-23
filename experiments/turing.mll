-- Universal Turing Machine
-- Simulates arbitrary Turing machines given their transition tables.
-- Demonstrates: ADTs with pattern matching, Maybe/case dispatch,
-- pure recursive simulation, zipper-based infinite tape

import Data.List (dropWhile)

-- Movement direction
data Dir = L | R | S

-- Transition rule: (from_state, read_symbol) → (to_state, write_symbol, direction)
data Rule = Rule Int Int Int Int Dir

-- Result of a TM run
data Result = Result Tape Int Int

resultTape :: Result -> Tape
resultTape (Result t _ _) = t

resultSteps :: Result -> Int
resultSteps (Result _ _ s) = s

-- Tape as zipper, blank symbol = 0
data Tape = Tape [Int] Int [Int]

mkTape :: [Int] -> Tape
mkTape []     = Tape [] 0 []
mkTape (x:xs) = Tape [] x xs

rdHead :: Tape -> Int
rdHead (Tape _ c _) = c

wrHead :: Int -> Tape -> Tape
wrHead s (Tape ls _ rs) = Tape ls s rs

move :: Dir -> Tape -> Tape
move R (Tape ls c [])     = Tape (c:ls) 0 []
move R (Tape ls c (r:rs)) = Tape (c:ls) r rs
move L (Tape [] c rs)     = Tape [] 0 (c:rs)
move L (Tape (l:ls) c rs) = Tape ls l (c:rs)
move S t                  = t

-- Look up the first matching rule for (state, symbol)
findRule :: [Rule] -> Int -> Int -> Maybe (Int, Int, Dir)
findRule [] _ _ = Nothing
findRule (Rule fs rs ts ws d : rest) st sym =
    if fs == st && rs == sym
    then Just (ts, ws, d)
    else findRule rest st sym

-- Extract tape contents trimming leading/trailing blanks
tapeToList :: Tape -> [Int]
tapeToList (Tape ls c rs) = trimBlanks (reverse ls ++ [c] ++ rs)

trimBlanks :: [Int] -> [Int]
trimBlanks xs = reverse (dropWhile (\x -> x == 0) (reverse (dropWhile (\x -> x == 0) xs)))

-- Count ones on the tape
countOnes :: [Int] -> Int
countOnes []     = 0
countOnes (1:xs) = 1 + countOnes xs
countOnes (_:xs) = countOnes xs

-- Run the machine until it halts or gets stuck
runTM :: [Rule] -> [Int] -> Int -> Tape -> Int -> Result
runTM rules halts state tape steps =
    if elem state halts
    then Result tape state steps
    else case findRule rules state (rdHead tape) of
        Nothing -> Result tape state steps
        Just (ns, ws, d) ->
            runTM rules halts ns (move d (wrHead ws tape)) (steps + 1)

-- ── 3-state Busy Beaver ──────────────────────────────────
-- States: 0=A  1=B  2=C  3=HALT
-- On a blank tape, writes 6 ones in 13 steps
busyBeaver :: [Rule]
busyBeaver =
    [ Rule 0 0  1 1 R    -- A,0 → 1,R,B
    , Rule 0 1  2 1 L    -- A,1 → 1,L,C
    , Rule 1 0  0 1 L    -- B,0 → 1,L,A
    , Rule 1 1  1 1 R    -- B,1 → 1,R,B
    , Rule 2 0  1 1 L    -- C,0 → 1,L,B
    , Rule 2 1  3 1 R    -- C,1 → 1,R,HALT
    ]

-- ── Binary increment (MSB-first) ────────────────────────
-- Symbols: 0=blank  1=bit₀  2=bit₁
-- States: 0=scan-right  1=carry  2=halt
binaryInc :: [Rule]
binaryInc =
    [ Rule 0 1  0 1 R    -- scan past 0-bits
    , Rule 0 2  0 2 R    -- scan past 1-bits
    , Rule 0 0  1 0 L    -- hit blank → carry
    , Rule 1 2  1 1 L    -- carry: 1+1 = 0, propagate
    , Rule 1 1  2 2 S    -- carry: 0+1 = 1, done
    , Rule 1 0  2 2 S    -- carry past MSB: new leading 1
    ]

-- ── Unary addition ──────────────────────────────────────
-- Input:  1^m 0 1^n   Output: 1^(m+n)
-- States: 0=fill-gap  1=scan-to-end  2=erase-last  3=halt
unaryAdd :: [Rule]
unaryAdd =
    [ Rule 0 1  0 1 R    -- scan first group
    , Rule 0 0  1 1 R    -- fill the gap
    , Rule 1 1  1 1 R    -- scan second group
    , Rule 1 0  2 0 L    -- past end → backtrack
    , Rule 2 1  3 0 S    -- erase last mark → halt
    ]

main :: IO ()
main = do
    -- 3-state Busy Beaver
    let bb = runTM busyBeaver [3] 0 (Tape [] 0 []) 0
    let ones = countOnes (tapeToList (resultTape bb))
    assert (ones == 6) ("BB(3) ones: expected 6, got " <> show ones)
    assert (resultSteps bb == 13) ("BB(3) steps: expected 13, got " <> show (resultSteps bb))
    putStrLn ("BB(3): " <> show ones <> " ones in " <> show (resultSteps bb) <> " steps")

    -- Binary increment: 101 (5) → 110 (6)
    let r1 = runTM binaryInc [2] 0 (mkTape [2, 1, 2]) 0
    assert (tapeToList (resultTape r1) == [2, 2, 1]) "inc 5→6"
    putStrLn ("inc 101 -> " <> show (tapeToList (resultTape r1)))

    -- Binary increment: 111 (7) → 1000 (8)
    let r2 = runTM binaryInc [2] 0 (mkTape [2, 2, 2]) 0
    assert (tapeToList (resultTape r2) == [2, 1, 1, 1]) "inc 7→8"
    putStrLn ("inc 111 -> " <> show (tapeToList (resultTape r2)))

    -- Binary increment: 1 (edge: single zero-bit) → 10
    let r3 = runTM binaryInc [2] 0 (mkTape [1]) 0
    assert (tapeToList (resultTape r3) == [2]) "inc 0→1"
    putStrLn ("inc 0   -> " <> show (tapeToList (resultTape r3)))

    -- Unary addition: 3 + 2 = 5
    let r4 = runTM unaryAdd [3] 0 (mkTape [1, 1, 1, 0, 1, 1]) 0
    assert (countOnes (tapeToList (resultTape r4)) == 5) "add 3+2"
    putStrLn ("3 + 2 = " <> show (countOnes (tapeToList (resultTape r4))))

    -- Unary addition: 1 + 1 = 2
    let r5 = runTM unaryAdd [3] 0 (mkTape [1, 0, 1]) 0
    assert (countOnes (tapeToList (resultTape r5)) == 2) "add 1+1"
    putStrLn ("1 + 1 = " <> show (countOnes (tapeToList (resultTape r5))))

    -- Unary addition: 4 + 0 = 4 (second group empty)
    let r6 = runTM unaryAdd [3] 0 (mkTape [1, 1, 1, 1, 0]) 0
    assert (countOnes (tapeToList (resultTape r6)) == 4) "add 4+0"
    putStrLn ("4 + 0 = " <> show (countOnes (tapeToList (resultTape r6))))

    putStrLn "turing: OK"
