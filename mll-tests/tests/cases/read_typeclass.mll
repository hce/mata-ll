-- Tests for Read typeclass

main :: IO ()
main = do
    -- read with type ascription
    let n = read "42" :: Int
    assert (n == 42) "read Int"

    let x = read "3.14" :: Number
    assert (x == 3.14) "read Number"

    let b = read "True" :: Bool
    assert (b == True) "read Bool True"

    let b2 = read "False" :: Bool
    assert (b2 == False) "read Bool False"

    -- read_Int directly
    assert (read_Int "100" == 100) "read_Int direct"
