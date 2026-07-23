-- GHC cgrun057: Caesar cipher on Int lists
-- Treat values as letter indices [0..25], shift by n

shiftLetter :: Int -> Int -> Int
shiftLetter n c = (c + n) `mod` 26

unshiftLetter :: Int -> Int -> Int
unshiftLetter n c = (c - n + 26) `mod` 26

encrypt :: Int -> [Int] -> [Int]
encrypt n cs = map (shiftLetter n) cs

decrypt :: Int -> [Int] -> [Int]
decrypt n cs = map (unshiftLetter n) cs

main :: IO ()
main = do
    -- h=7 e=4 l=11 l=11 o=14
    let msg = [7, 4, 11, 11, 14]
    let enc3 = encrypt 3 msg
    -- shifted: 10 7 14 14 17
    assert (enc3 == [10, 7, 14, 14, 17]) "encrypt by 3"
    assert (decrypt 3 enc3 == msg) "decrypt roundtrip"

    -- ROT13
    let enc13 = encrypt 13 msg
    assert (decrypt 13 enc13 == msg) "ROT13 roundtrip"
    assert (encrypt 13 (encrypt 13 msg) == msg) "ROT13 twice is identity"

    -- shift by 0 is identity
    assert (encrypt 0 msg == msg) "shift 0 is identity"
    -- wrap-around: z (25) + 1 = a (0)
    assert (encrypt 1 [25] == [0]) "25+1 wraps to 0"
    assert (decrypt 1 [0] == [25]) "0-1 wraps to 25"
    putStrLn "ok"
