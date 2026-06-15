-- Tests for Enum typeclass and range syntax

main :: IO ()
main = do
    -- Basic Enum methods
    assert (succ 5 == 6) "succ"
    assert (pred 5 == 4) "pred"
    assert (toEnum 42 == 42) "toEnum"
    assert (fromEnum 42 == 42) "fromEnum"

    -- Range syntax: [x..y]
    assert ([1..5] == [1, 2, 3, 4, 5]) "enumFromTo"
    assert ([5..5] == [5]) "enumFromTo single"
    assert ([5..3] == []) "enumFromTo empty"

    -- Range with step: [x,y..z]
    assert ([1,3..10] == [1, 3, 5, 7, 9]) "enumFromThenTo step 2"
    assert ([0,5..20] == [0, 5, 10, 15, 20]) "enumFromThenTo step 5"
    assert ([10,8..2] == [10, 8, 6, 4, 2]) "enumFromThenTo countdown"
    assert ([10,8..11] == []) "enumFromThenTo empty down"

    -- Infinite range: [x..] (take a finite prefix)
    assert (take 5 [1..] == [1, 2, 3, 4, 5]) "enumFrom take"
    assert (take 3 [10..] == [10, 11, 12]) "enumFrom take 10"

    -- Infinite range with step: [x,y..]
    assert (take 4 [1,3..] == [1, 3, 5, 7]) "enumFromThen take"
    assert (take 3 [0,10..] == [0, 10, 20]) "enumFromThen step 10"
