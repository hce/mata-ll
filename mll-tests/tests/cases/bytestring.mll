-- ByteString intrinsic type tests

main :: IO ()
main = do
    -- Empty and null
    assert (bsNull bsEmpty) "bsEmpty is null"
    assert (bsLength bsEmpty == 0) "bsEmpty length"

    -- Singleton
    let one = bsSingleton 65
    assert (bsLength one == 1) "singleton length"
    assert (bsHead one == 65) "singleton head is 65 (A)"
    assert (not (bsNull one)) "singleton not null"

    -- Cons and Snoc
    let ab = bsCons 65 (bsSingleton 66)
    assert (bsLength ab == 2) "cons length"
    assert (bsHead ab == 65) "cons head"
    assert (bsHead (bsTail ab) == 66) "cons tail head"

    let ba = bsSnoc (bsSingleton 65) 66
    assert (bsLength ba == 2) "snoc length"
    assert (bsHead ba == 65) "snoc head"

    -- Index (0-based)
    assert (bsIndex ab 0 == 65) "index 0"
    assert (bsIndex ab 1 == 66) "index 1"

    -- Sub (offset, length)
    let hello = bsFromString "hello world"
    assert (bsToString (bsSub hello 0 5) == "hello") "sub first word"
    assert (bsToString (bsSub hello 6 5) == "world") "sub second word"
    assert (bsToString (bsSub hello 0 0) == "") "sub empty"

    -- Concat
    let hw = bsConcat (bsFromString "hello") (bsFromString " world")
    assert (bsToString hw == "hello world") "concat"

    -- ConcatList
    let parts = [bsFromString "a", bsFromString "b", bsFromString "c"]
    assert (bsToString (bsConcatList parts) == "abc") "concatList"

    -- Pack and Unpack roundtrip
    let bytes = [72, 105]
    let packed = bsPack bytes
    assert (bsToString packed == "Hi") "pack to string"
    assert (bsUnpack packed == [72, 105]) "unpack roundtrip"

    -- Replicate
    let rep = bsReplicate 3 42
    assert (bsLength rep == 3) "replicate length"
    assert (bsIndex rep 0 == 42) "replicate value 0"
    assert (bsIndex rep 2 == 42) "replicate value 2"

    -- Head and Tail
    let abc = bsFromString "abc"
    assert (bsHead abc == 97) "head a"
    assert (bsToString (bsTail abc) == "bc") "tail bc"
    assert (bsToString (bsTail (bsTail abc)) == "c") "tail tail c"

    -- ToString / FromString roundtrip
    let orig = "test string"
    assert (bsToString (bsFromString orig) == orig) "string roundtrip"

    -- Map: increment each byte
    let mapped = bsMap (+ 1) (bsFromString "abc")
    assert (bsToString mapped == "bcd") "map increment"

    -- Foldl: sum of bytes
    let bs = bsPack [10, 20, 30]
    let total = bsFoldl (\acc b -> acc + b) 0 bs
    assert (total == 60) "foldl sum"

    -- XOR
    let x1 = bsPack [255, 0, 170]
    let x2 = bsPack [255, 255, 85]
    let xored = bsXor x1 x2
    assert (bsIndex xored 0 == 0) "xor ff^ff = 0"
    assert (bsIndex xored 1 == 255) "xor 00^ff = ff"
    assert (bsIndex xored 2 == 255) "xor aa^55 = ff"

    -- ZipWith: add corresponding bytes
    let z1 = bsPack [1, 2, 3]
    let z2 = bsPack [10, 20, 30]
    let zipped = bsZipWith (\a b -> a + b) z1 z2
    assert (bsUnpack zipped == [11, 22, 33]) "zipWith add"

    -- Binary read: little-endian u16
    let le16 = bsPack [0, 1]
    assert (bsGetU16LE le16 0 == 256) "getU16LE 0x0100"

    -- Binary read: little-endian u32
    let le32 = bsPack [1, 0, 0, 0]
    assert (bsGetU32LE le32 0 == 1) "getU32LE 1"
    let le32b = bsPack [0, 1, 0, 0]
    assert (bsGetU32LE le32b 0 == 256) "getU32LE 256"

    -- Binary read: signed i8
    let pos = bsPack [127]
    assert (bsGetI8 pos 0 == 127) "getI8 positive"
    let neg = bsPack [255]
    assert (bsGetI8 neg 0 == -1) "getI8 negative (0xFF = -1)"
    let neg128 = bsPack [128]
    assert (bsGetI8 neg128 0 == -128) "getI8 -128"

    -- Binary read: signed i16 LE
    let i16pos = bsPack [1, 0]
    assert (bsGetI16LE i16pos 0 == 1) "getI16LE positive"
    let i16neg = bsPack [255, 255]
    assert (bsGetI16LE i16neg 0 == -1) "getI16LE negative"

    -- Binary write: i16 LE
    let written = bsPutI16LE 256
    assert (bsIndex written 0 == 0) "putI16LE 256 lo"
    assert (bsIndex written 1 == 1) "putI16LE 256 hi"

    -- Show instance
    let shown = show (bsPack [171, 205])
    assert (shown == "ByteString abcd") "show bytestring"

    -- Eq instance
    assert (bsFromString "abc" == bsFromString "abc") "bs eq"
    assert (bsFromString "abc" /= bsFromString "abd") "bs neq"
    assert (bsEmpty == bsEmpty) "bs empty eq"

    -- Ord instance (byte-lexicographic, same as Lua string comparison)
    assert (bsFromString "abc" < bsFromString "abd") "bs lt"
    assert (bsFromString "abd" > bsFromString "abc") "bs gt"
    assert (bsFromString "abc" <= bsFromString "abc") "bs le"
    assert (bsFromString "abd" >= bsFromString "abc") "bs ge"
    assert (compare (bsFromString "abc") (bsFromString "abd") == LT) "bs compare LT"
    assert (compare (bsFromString "abd") (bsFromString "abc") == GT) "bs compare GT"
    assert (compare (bsFromString "abc") (bsFromString "abc") == EQ) "bs compare EQ"
    assert (bsEmpty < bsFromString "a") "bs empty lt"
