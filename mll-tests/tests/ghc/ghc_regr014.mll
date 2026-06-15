-- ghc_regr014: Tuple of tuples, nested tuple access

-- Nested tuple type aliases (by usage)
swap :: (a, b) -> (b, a)
swap (x, y) = (y, x)

-- Nested tuple: access inner components via pattern matching
innerFst :: ((a, b), c) -> a
innerFst ((x, _), _) = x

innerSnd :: ((a, b), c) -> b
innerSnd ((_, y), _) = y

outer :: ((a, b), c) -> c
outer (_, z) = z

-- Triple-nested
deep :: (((Integer, Integer), Integer), Integer) -> Integer
deep (((a, b), c), d) = a + b + c + d

-- Tuple of tuples in a list
type Point = (Number, Number)
type Segment = (Point, Point)

midpoint :: Segment -> Point
midpoint ((x1, y1), (x2, y2)) =
    ((x1 + x2) / 2.0, (y1 + y2) / 2.0)

segmentLength :: Segment -> Number
segmentLength ((x1, y1), (x2, y2)) =
    let dx = x2 - x1
        dy = y2 - y1
    in sqrt (dx * dx + dy * dy)

-- 3-tuple operations
fst3 :: (a, b, c) -> a
fst3 (x, _, _) = x

snd3 :: (a, b, c) -> b
snd3 (_, y, _) = y

thd3 :: (a, b, c) -> c
thd3 (_, _, z) = z

main :: IO ()
main = do
    -- Basic nested tuple access
    let t = ((1, 2), 3) :: ((Integer, Integer), Integer)
    assert (innerFst t == 1) "innerFst"
    assert (innerSnd t == 2) "innerSnd"
    assert (outer t == 3) "outer"

    -- swap
    assert (swap (1, "a") == ("a", 1)) "swap"
    assert (swap (swap (True, 42)) == (True, 42)) "swap twice"

    -- Triple nested
    assert (deep (((1, 2), 3), 4) == 10) "deep"
    assert (deep (((10, 20), 30), 40) == 100) "deep 100"

    -- Midpoint
    let seg = ((0.0, 0.0), (4.0, 6.0)) :: Segment
    let mid = midpoint seg
    assert (fst mid == 2.0) "midpoint x"
    assert (snd mid == 3.0) "midpoint y"

    -- Segment length: 3-4-5 triangle
    let seg2 = ((0.0, 0.0), (3.0, 4.0)) :: Segment
    let diff = segmentLength seg2 - 5.0
    assert (diff * diff < 0.0001) "length 5"

    -- 3-tuple
    let triple = (10, "hello", True)
    assert (fst3 triple == 10) "fst3"
    assert (snd3 triple == "hello") "snd3"
    assert (thd3 triple == True) "thd3"

    -- List of tuples of tuples
    let segments = [((0.0, 0.0), (1.0, 0.0)), ((0.0, 0.0), (0.0, 1.0))] :: [Segment]
    assert (length segments == 2) "segments length"
    assert (fst (fst (head segments)) == 0.0) "nested access in list"

    putStrLn "ok"
