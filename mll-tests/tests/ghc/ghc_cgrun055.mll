-- GHC cgrun055: Run-length encoding of a list

data Run = Run Integer Integer
    deriving (Show, Eq)

encode :: [Integer] -> [Run]
encode [] = []
encode (x:xs) = encodeHelper x 1 xs

encodeHelper :: Integer -> Integer -> [Integer] -> [Run]
encodeHelper c n [] = [Run c n]
encodeHelper c n (x:xs)
    | c == x    = encodeHelper c (n + 1) xs
    | otherwise = Run c n : encodeHelper x 1 xs

decode :: [Run] -> [Integer]
decode [] = []
decode (Run c n : rest) = rep c n (decode rest)

rep :: Integer -> Integer -> [Integer] -> [Integer]
rep _ 0 acc = acc
rep x n acc = x : rep x (n - 1) acc

runCount :: Run -> Integer
runCount (Run _ n) = n

main :: IO ()
main = do
    let xs = [1,1,2,3,3,3,4,4,1,1,1]
    let enc = encode xs
    assert (enc == [Run 1 2, Run 2 1, Run 3 3, Run 4 2, Run 1 3]) "encode"
    assert (decode enc == xs) "decode roundtrip"
    assert (encode [] == []) "encode empty"
    assert (decode [] == []) "decode empty"
    assert (encode [1] == [Run 1 1]) "encode single"
    let allSame = [7,7,7,7,7]
    assert (encode allSame == [Run 7 5]) "all same"
    assert (decode (encode allSame) == allSame) "all same roundtrip"
    let lens = map runCount (encode xs)
    assert (foldl (+) 0 lens == length xs) "lengths sum"
    putStrLn "ok"
