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

    -- Validation (round-3 Q52): read rejects what the type's grammar
    -- does not cover, with GHC's "no parse" error — the old readers
    -- accepted garbage (read @Int "3.5" returned a fraction, "junk"
    -- returned nil; read @Bool mapped anything non-True to False;
    -- read @Integer mapped arbitrary bytes through byte-48).
    let failed r = case r of
            Left _ -> True
            Right _ -> False
    r1 <- try (pure (read "3.5" :: Int) >>= \v -> print v)
    assert (failed r1) "read Int rejects a fraction"
    r2 <- try (pure (read "junk" :: Int) >>= \v -> print v)
    assert (failed r2) "read Int rejects junk"
    r3 <- try (pure (read "yes" :: Bool) >>= \v -> print v)
    assert (failed r3) "read Bool rejects non-True/False"
    r4 <- try (pure (read "12x" :: Integer) >>= \v -> print v)
    assert (failed r4) "read Integer rejects trailing junk"
    r5 <- try (pure (read "3." :: Number) >>= \v -> print v)
    assert (failed r5) "read Number rejects a bare trailing dot"
    -- accept side of the sharpened grammars
    assert ((read " (-7) " :: Int) == (-7)) "read Int parens and spaces"
    assert ((read "2e3" :: Number) == 2000.0) "read Number exponent form"
