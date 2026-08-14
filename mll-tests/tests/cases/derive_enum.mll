-- Tests for deriving Enum and Bounded on user-defined types

data Color = Red | Green | Blue
    deriving (Show, Eq, Enum, Bounded)

data Priority = Low | Medium | High | Critical
    deriving (Show, Eq, Ord, Enum, Bounded)

data Bool2 = No | Yes
    deriving (Show, Eq, Enum, Bounded)

main :: IO ()
main = do
    -- fromEnum / toEnum
    assert (fromEnum Red == 0) "fromEnum Red"
    assert (fromEnum Blue == 2) "fromEnum Blue"
    assert (toEnum 1 == Green) "toEnum 1 Green"
    assert (toEnum 0 == Red) "toEnum 0 Red"

    -- succ / pred
    assert (succ Red == Green) "succ Red"
    assert (succ Green == Blue) "succ Green"
    assert (pred Blue == Green) "pred Blue"
    assert (pred Green == Red) "pred Green"

    -- Range syntax: enumFromTo
    assert ([Red .. Blue] == [Red, Green, Blue]) "enumFromTo full"
    assert ([Green .. Blue] == [Green, Blue]) "enumFromTo partial"
    assert ([Blue .. Blue] == [Blue]) "enumFromTo single"
    assert ([Blue .. Red] == []) "enumFromTo empty"

    -- Range syntax: enumFrom (bounded)
    assert ([Red ..] == [Red, Green, Blue]) "enumFrom Red"
    assert ([Green ..] == [Green, Blue]) "enumFrom Green"
    assert ([Blue ..] == [Blue]) "enumFrom Blue"

    -- Bounded
    assert (minBound == Red) "minBound Color"
    assert (maxBound == Blue) "maxBound Color"

    -- 4-element type
    assert ([Low .. Critical] == [Low, Medium, High, Critical]) "enumFromTo Priority"
    assert (fromEnum Critical == 3) "fromEnum Critical"
    assert (succ Medium == High) "succ Medium"
    assert (pred High == Medium) "pred High"
    assert (minBound == Low) "minBound Priority"
    assert (maxBound == Critical) "maxBound Priority"

    -- 2-element type (minimal)
    assert ([No .. Yes] == [No, Yes]) "enumFromTo Bool2"
    assert (succ No == Yes) "succ No"
    assert (pred Yes == No) "pred Yes"
    assert (fromEnum No == 0) "fromEnum No"
    assert (fromEnum Yes == 1) "fromEnum Yes"

    -- Range syntax: enumFromThen (limit comes from the step direction:
    -- maxBound when ascending, minBound when descending — GHC semantics)
    assert ([Low, High ..] == [Low, High]) "enumFromThen step 2"
    assert ([Low, Medium ..] == [Low, Medium, High, Critical]) "enumFromThen step 1"
    assert ([Low, Critical ..] == [Low, Critical]) "enumFromThen step 3"
    assert ([Critical, High ..] == [Critical, High, Medium, Low]) "enumFromThen step -1"
    assert ([Critical, Medium ..] == [Critical, Medium]) "enumFromThen step -2"
    assert ([Medium, Low ..] == [Medium, Low]) "enumFromThen down to minBound"
    -- A zero step repeats the start element forever, as for [x, x ..] :: [Int]
    assert (take 3 [Green, Green ..] == [Green, Green, Green]) "enumFromThen step 0"

    -- Range syntax: enumFromThenTo
    assert ([Low, Medium .. High] == [Low, Medium, High]) "enumFromThenTo step 1"
    assert ([Low, High .. Critical] == [Low, High]) "enumFromThenTo step 2"
    assert ([High, Medium .. Low] == [High, Medium, Low]) "enumFromThenTo step -1"
    assert ([Low, High .. Medium] == [Low]) "enumFromThenTo overshoot"
    assert ([Critical, High .. Critical] == [Critical]) "enumFromThenTo single down"
    assert ([Medium, High .. Low] == []) "enumFromThenTo empty up"

    -- Enum + Ord interaction
    assert (Low < Critical) "Ord Low < Critical"
    assert (Critical > Medium) "Ord Critical > Medium"

    putStrLn "."
