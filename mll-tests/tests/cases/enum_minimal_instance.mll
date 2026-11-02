-- GHC's Enum defaults count through Int: an instance that gives toEnum
-- and fromEnum — the minimal complete definition — has succ, pred and
-- the four range forms, each `map toEnum` over the Int range. Unlike a
-- derived instance, `[x ..]` is unbounded above (it is toEnum that
-- stops it), so it is consumed with take.

data C = Red | Green | Blue deriving (Show, Eq)

instance Enum C where
    toEnum 0 = Red
    toEnum 1 = Green
    toEnum 2 = Blue
    toEnum n = error ("toEnum C: " <> show n)
    fromEnum Red = 0
    fromEnum Green = 1
    fromEnum Blue = 2

main :: IO ()
main = do
    print (succ Red, succ Green, pred Blue, pred Green)
    print (map fromEnum [Red, Green, Blue], toEnum 1 :: C)
    print (take 3 [Red ..], take 2 [Green ..])
    print ([Red .. Blue], [Green .. Blue], [Blue .. Red])
    print (take 2 [Red, Blue ..], [Blue, Green .. Red], [Red, Green .. Blue], [Blue, Red .. Red])
    print (zip [Red ..] [10, 20, 30], [c | c <- [Red .. Blue], c /= Green])
