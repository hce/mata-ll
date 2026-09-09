-- floor / ceiling / truncate / round with GHC's names and semantics
-- (round is half to even). mata-ll fixes the result type at Int (the
-- RealFrac generalization is deferred); under GHC the results default
-- to Integer — the printed output is identical.

main :: IO ()
main = do
    print (floor 2.7)
    print (floor (-2.5))
    print (floor 2.0)
    print (ceiling 2.3)
    print (ceiling (-2.5))
    print (ceiling 4.0)
    print (truncate 2.7)
    print (truncate (-2.7))
    print (truncate 0.9)
    print (truncate (-0.9))
    -- Ties go to the even neighbour.
    print (round 2.5)
    print (round 3.5)
    print (round (-2.5))
    print (round (-3.5))
    print (round 0.5)
    print (round (-0.5))
    print (round 1.5)
    -- Non-ties round to nearest.
    print (round 2.4)
    print (round 2.6)
    print (round (-2.4))
    print (round (-2.6))
    -- Integral doubles round to themselves, including past 2^52.
    print (round 1000000.0)
    print (round 9007199254740994.0)
    print (floor 9007199254740994.0)
    -- Results are Ints: they compute.
    print (floor 2.7 + ceiling 2.3 + truncate (-2.7) + round 2.5)

-- expect: 2
-- expect: -3
-- expect: 2
-- expect: 3
-- expect: -2
-- expect: 4
-- expect: 2
-- expect: -2
-- expect: 0
-- expect: 0
-- expect: 2
-- expect: 4
-- expect: -2
-- expect: -4
-- expect: 0
-- expect: 0
-- expect: 2
-- expect: 2
-- expect: 3
-- expect: -2
-- expect: -3
-- expect: 1000000
-- expect: 9007199254740994
-- expect: 9007199254740994
-- expect: 5
