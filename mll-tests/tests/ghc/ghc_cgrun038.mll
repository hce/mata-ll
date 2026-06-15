-- GHC cgrun038: State machine simulation
-- Tests ADTs as state, pattern matching for transitions

data State = Locked | Unlocked
    deriving (Show, Eq)

data Input = Coin | Push
    deriving (Show, Eq)

data Output = Thank | Open | Tut
    deriving (Show, Eq)

step :: State -> Input -> (State, Output)
step Locked   Coin = (Unlocked, Thank)
step Locked   Push = (Locked, Tut)
step Unlocked Coin = (Unlocked, Thank)
step Unlocked Push = (Locked, Open)

run_ :: State -> [Input] -> [(Output, State)]
run_ _ [] = []
run_ s (i:is) = (o, s') : run_ s' is
  where
    result = step s i
    s' = fst result
    o = snd result

main :: IO ()
main = do
    -- Basic transitions
    assert (step Locked Coin == (Unlocked, Thank)) "locked coin"
    assert (step Locked Push == (Locked, Tut)) "locked push"
    assert (step Unlocked Push == (Locked, Open)) "unlocked push"

    -- Sequence
    let trace = run_ Locked [Coin, Push, Push, Coin, Push]
    assert (length trace == 5) "trace length"
    assert (map fst trace == [Thank, Open, Tut, Thank, Open]) "trace outputs"
    assert (map snd trace == [Unlocked, Locked, Locked, Unlocked, Locked]) "trace states"

    putStrLn "ok"
