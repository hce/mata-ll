-- Tests for Read typeclass

main :: IO ()
main = do
    -- read with type ascription
    let n = read "42" :: Integer
    assert (n == 42) "read Integer"

    let x = read "3.14" :: Number
    assert (x == 3.14) "read Number"

    let b = read "True" :: Bool
    assert (b == True) "read Bool True"

    let b2 = read "False" :: Bool
    assert (b2 == False) "read Bool False"

    -- read_Integer directly
    assert (read_Integer "100" == 100) "read_Integer direct"
