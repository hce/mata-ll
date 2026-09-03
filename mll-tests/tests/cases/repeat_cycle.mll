-- `repeat` and `cycle` (GHC Prelude): infinite lists as one shared cyclic
-- cell / a tied knot. Both were missing (HASKDIFF cited `or (repeat True)`
-- for a function that did not exist).

main :: IO ()
main = do
    print (take 3 (repeat "x"))
    print (take 7 (cycle [1, 2, 3 :: Int]))
    print (take 2 (repeat [True]))
    print (or (repeat True))
    print (zip (cycle ["a", "b"]) [1, 2, 3, 4, 5 :: Int])
    print (takeWhile (< 10) (map (* 3) (cycle [1, 2, 5 :: Int])))
    print (length (take 100000 (repeat (0 :: Int))))
