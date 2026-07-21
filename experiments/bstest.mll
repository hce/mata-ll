-- ByteString basic tests

main :: IO ()
main = do
    -- Construction
    let empty = bsEmpty
    assert (bsNull empty) "bsNull empty"
    assert (bsLength empty == 0) "bsLength empty"

    let one = bsSingleton 65
    assert (bsLength one == 1) "bsSingleton length"
    assert (bsHead one == 65) "bsSingleton head"

    -- bsPack / bsUnpack roundtrip
    let bs = bsPack [72, 101, 108, 108, 111]
    assert (bsLength bs == 5) "bsPack length"
    assert (bsIndex bs 0 == 72) "bsIndex 0"
    assert (bsIndex bs 4 == 111) "bsIndex 4"
    assert (bsToString bs == "Hello") "bsToString"

    -- bsUnpack
    let unpacked = bsUnpack bs
    assert (head unpacked == 72) "bsUnpack head"

    -- bsSub (0-based offset, length)
    let sub = bsSub bs 1 3
    assert (bsToString sub == "ell") "bsSub"

    -- bsConcat
    let world = bsFromString " World"
    let hw = bsConcat bs world
    assert (bsToString hw == "Hello World") "bsConcat"

    -- bsCons / bsSnoc
    let bang = bsSnoc hw 33
    assert (bsToString bang == "Hello World!") "bsSnoc"
    let prefixed = bsCons 62 bs
    assert (bsHead prefixed == 62) "bsCons head"

    -- bsTail
    let tl = bsTail bs
    assert (bsToString tl == "ello") "bsTail"

    -- bsReplicate
    let rep = bsReplicate 4 0
    assert (bsLength rep == 4) "bsReplicate length"
    assert (bsIndex rep 0 == 0) "bsReplicate byte"

    -- bsMap (increment each byte)
    let mapped = bsMap (\b -> b + 1) bs
    assert (bsHead mapped == 73) "bsMap"

    -- bsXor
    let a = bsPack [255, 0, 170, 85]
    let b = bsPack [255, 255, 255, 255]
    let x = bsXor a b
    assert (bsIndex x 0 == 0) "bsXor 0"
    assert (bsIndex x 1 == 255) "bsXor 1"
    assert (bsIndex x 2 == 85) "bsXor 2"
    assert (bsIndex x 3 == 170) "bsXor 3"

    -- bsZipWith
    let zipped = bsZipWith (\x y -> x + y) (bsPack [1, 2, 3]) (bsPack [10, 20, 30])
    assert (bsIndex zipped 0 == 11) "bsZipWith 0"
    assert (bsIndex zipped 2 == 33) "bsZipWith 2"

    -- bsFoldl (sum of bytes)
    let sum = bsFoldl (\acc b -> acc + b) 0 (bsPack [1, 2, 3, 4])
    assert (sum == 10) "bsFoldl sum"

    -- Eq
    assert (bsPack [1, 2, 3] == bsPack [1, 2, 3]) "ByteString Eq true"
    assert (bsPack [1, 2, 3] /= bsPack [1, 2, 4]) "ByteString Eq false"

    -- Show
    putStrLn (show (bsPack [222, 173, 190, 239]))

    putStrLn "All ByteString tests passed!"
