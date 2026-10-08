sumTo :: Int -> Int
sumTo n = let go 0 acc = acc
              go k acc = go (k - 1) (acc + k)
          in go n 0

describe :: Maybe Int -> String
describe m = render m
  where
    render Nothing = "nothing"
    render (Just 0) = "just zero"
    render (Just k) = "just " <> show k

parity :: Int -> String
parity n = let isEven 0 = True
               isEven k = isOdd (k - 1)
               isOdd 0 = False
               isOdd k = isEven (k - 1)
           in if isEven n then "even" else "odd"

sized :: [Int] -> String
sized xs = let label :: [Int] -> String
               label [] = "empty"
               label [_] = "one"
               label _ = "many"
           in label xs

twice :: (Int, Bool)
twice = let dup [] = []
            dup (x:rest) = x : x : dup rest
        in (length (dup [1, 2, 3 :: Int]), null (dup [True]))

main :: IO ()
main = do
    let f 0 = "zero"
        f _ = "other"
    putStrLn (f (0 :: Int))
    putStrLn (f 1)
    putStrLn (let g True = "yes"
                  g False = "no"
              in g True)
    print (let go 0 acc = acc; go n acc = go (n - 1) (acc + n) in go (10 :: Int) 0 + 1)
    print (sumTo 100)
    putStrLn (describe Nothing)
    putStrLn (describe (Just 0))
    putStrLn (describe (Just 7))
    putStrLn (parity 10)
    putStrLn (parity 7)
    putStrLn (sized [])
    putStrLn (sized [1])
    putStrLn (sized [1, 2])
    print twice
    let (lo, hi) = (1 :: Int, 9 :: Int)
        mid x = (lo + hi) `div` x
    print (mid 2)
    let h (Just x) y = x + y
        h Nothing y = y
    print (h (Just 3) (4 :: Int))
    print (h Nothing (4 :: Int))
