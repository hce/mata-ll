-- GHC tc009: Eq/Ord interaction
-- Using both == and < on the same values; verifying constraint combination

data Priority = Low | Medium | High
    deriving (Show, Eq, Ord)

data Task = Task
    { taskName     :: String
    , taskPriority :: Priority
    , taskDone     :: Bool
    }
    deriving (Show, Eq)

-- Uses both Eq and Ord on Priority
rankTask :: Task -> Int
rankTask t
    | taskDone t        = 0
    | taskPriority t == High   = 3
    | taskPriority t == Medium = 2
    | otherwise                = 1

-- Requires Eq and Ord together
between :: Ord a => a -> a -> a -> Bool
between lo hi x = lo <= x && x <= hi

sortedPair :: Ord a => a -> a -> (a, a)
sortedPair a b
    | a <= b    = (a, b)
    | otherwise = (b, a)

maxOfThree :: Ord a => a -> a -> a -> a
maxOfThree a b c
    | a >= b && a >= c = a
    | b >= c           = b
    | otherwise        = c

main :: IO ()
main = do
    -- Eq checks
    assert (Low == Low) "low eq"
    assert (Low /= High) "low ne high"

    -- Ord checks
    assert (Low < High) "low lt high"
    assert (High > Medium) "high gt medium"
    assert (Medium >= Medium) "medium ge medium"
    assert (Low <= Medium) "low le medium"

    -- between uses both <= (Ord)
    assert (between Low High Medium == True)  "between mid"
    assert (between Low Medium High == False) "between out"
    assert (between Low Low Low == True)      "between edge"

    -- sortedPair
    let sp = sortedPair High Low
    assert (fst sp == Low)  "sorted fst"
    assert (snd sp == High) "sorted snd"

    -- maxOfThree
    assert (maxOfThree Low Medium High == High) "max three"
    assert (maxOfThree High Low Medium == High) "max three 2"

    -- Task rank uses both == and guards
    let t1 = Task { taskName = "x", taskPriority = High,   taskDone = False }
    let t2 = Task { taskName = "y", taskPriority = Medium, taskDone = True  }
    assert (rankTask t1 == 3) "high undone"
    assert (rankTask t2 == 0) "done"

    putStrLn "ok"
