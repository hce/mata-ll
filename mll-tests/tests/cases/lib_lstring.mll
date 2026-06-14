import LString (strByte, strLen, strSub, strChar)

main :: IO ()
main = do
    -- strLen
    assert (strLen "" == 0) "strLen empty"
    assert (strLen "a" == 1) "strLen single"
    assert (strLen "hello" == 5) "strLen hello"

    -- strByte (Lua uses 1-based indexing)
    assert (strByte "A" 1 == 65) "strByte A"
    assert (strByte "abc" 1 == 97) "strByte a"
    assert (strByte "abc" 2 == 98) "strByte b"
    assert (strByte "abc" 3 == 99) "strByte c"
    assert (strByte "0" 1 == 48) "strByte 0"
    assert (strByte " " 1 == 32) "strByte space"
    assert (strByte "\n" 1 == 10) "strByte newline"

    -- strChar
    assert (strChar 65 == "A") "strChar 65 -> A"
    assert (strChar 97 == "a") "strChar 97 -> a"
    assert (strChar 48 == "0") "strChar 48 -> 0"
    assert (strChar 10 == "\n") "strChar 10 -> newline"

    -- strByte/strChar roundtrip
    assert (strChar (strByte "Z" 1) == "Z") "byte/char roundtrip Z"
    assert (strChar (strByte "!" 1) == "!") "byte/char roundtrip !"

    -- strSub (1-based, inclusive both ends)
    assert (strSub "hello" 1 5 == "hello") "strSub full"
    assert (strSub "hello" 1 1 == "h") "strSub first"
    assert (strSub "hello" 5 5 == "o") "strSub last"
    assert (strSub "hello" 2 4 == "ell") "strSub middle"
    assert (strSub "abcdef" 3 5 == "cde") "strSub range"
    assert (strSub "hello" 1 0 == "") "strSub empty range"
