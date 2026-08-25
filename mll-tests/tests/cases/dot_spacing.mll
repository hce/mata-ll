-- The `.` disambiguation between composition and OverloadedRecordDot
-- field access is ADJACENCY ON BOTH SIDES.  Regression: only the left
-- side was checked, so `negate. abs` (space after the dot) parsed as
-- the field access `abs negate` instead of the composition GHC reads —
-- surfacing as a baffling Num-instance error.  `p.px` (hugging both
-- sides) stays field access.  (`negate .abs` is deliberately not here:
-- GHC's OverloadedRecordDot parse-errors on that spelling, while
-- mata-ll reads it as composition — a pre-existing laxness outside
-- this regression.)

data P = P { px :: Int, py :: Int }

f :: Int -> Int
f = negate. abs

spaced :: Int -> Int
spaced = negate . abs

main :: IO ()
main = do
    print (f 3)
    print (spaced 5)
    let p = P { px = 7, py = 8 }
    print p.px
    print (p.py + 1)
