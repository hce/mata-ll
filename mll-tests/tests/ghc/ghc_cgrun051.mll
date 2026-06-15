-- GHC cgrun051: Binary encoding/decoding (Integer to/from list of bits)

toBitsHelper :: Integer -> [Integer]
toBitsHelper 0 = []
toBitsHelper n = (n `mod` 2) : toBitsHelper (n `div` 2)

toBits :: Integer -> [Integer]
toBits 0 = [0]
toBits n = reverse (toBitsHelper n)

fromBits :: [Integer] -> Integer
fromBits bits = foldl (\acc b -> acc * 2 + b) 0 bits

main :: IO ()
main = do
    assert (toBits 0 == [0]) "0 -> [0]"
    assert (toBits 1 == [1]) "1 -> [1]"
    assert (toBits 5 == [1, 0, 1]) "5 -> [1,0,1]"
    assert (toBits 10 == [1, 0, 1, 0]) "10 -> [1,0,1,0]"
    assert (toBits 255 == [1,1,1,1,1,1,1,1]) "255 -> 8 ones"
    assert (fromBits [1, 0, 1] == 5) "bits -> 5"
    assert (fromBits [1, 0, 1, 0] == 10) "bits -> 10"
    assert (fromBits (toBits 42) == 42) "roundtrip 42"
    assert (fromBits (toBits 127) == 127) "roundtrip 127"
    assert (fromBits (toBits 1000) == 1000) "roundtrip 1000"
    putStrLn "ok"
