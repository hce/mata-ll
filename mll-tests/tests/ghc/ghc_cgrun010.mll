-- GHC cgrun010: Tuple pattern binding

fst_ :: (Int, Int) -> Int
fst_ (a, b) = a

main :: IO ()
main = do
    let pair = (3 + 4, 5 + 6)
    putStrLn (show (fst_ pair))
