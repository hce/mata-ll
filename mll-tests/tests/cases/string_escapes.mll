-- GHC-parity string escape decoding on the INPUT (lexer) side.
--
-- mata-ll's String is the Lua string — a byte array with no encoding
-- awareness (HASKDIFF.md, "Strings and ByteStrings"). Every escape below is
-- checked by the BYTE it produces (strByte, 1-based) and by the number of
-- bytes it contributes (strLen), so this is a differential check against the
-- byte values GHC would produce, computed from the Haskell 2010 Report §2.6.
-- The `show` side is already byte-verified against GHC's showLitString; the
-- final block below asserts read . show == id for the byte-escape cases, i.e.
-- the input and output halves agree.
--
-- CANNOT run GHC locally, so these are self-checking asserts on the Report's
-- byte values rather than new goldens (see run_mll.rs ghc_oracle notes).

import LString (strByte, strLen)

main :: IO ()
main = do
    -- Shorthand control escapes (GHC's charesc). \n \t \r \\ \" already worked;
    -- these are the newly-added ones.
    assert (strByte "\a" 1 == 7) "bell \\a == 7"
    assert (strByte "\b" 1 == 8) "backspace \\b == 8"
    assert (strByte "\f" 1 == 12) "formfeed \\f == 12"
    assert (strByte "\v" 1 == 11) "vtab \\v == 11"
    assert (strByte "\'" 1 == 39) "escaped quote \\' == 39"

    -- Decimal numeric escapes, MAXIMAL MUNCH. The bug this fixes: mata-ll's
    -- old \0 stopped after one digit, so "\05" decoded to ['\0','5']. GHC (and
    -- now mata-ll) reads the full digit run, so "\05" is a single byte 5.
    assert (strLen "\05" == 1) "maximal munch: \"\\05\" is one byte"
    assert (strByte "\05" 1 == 5) "maximal munch: \"\\05\" is byte 5"
    assert (strByte "\0" 1 == 0) "\\0 still decodes to NUL"
    assert (strByte "\181" 1 == 181) "decimal \\181 == 181"
    assert (strLen "\181" == 1) "decimal \\181 is one byte (not UTF-8)"

    -- Octal (\o) and hex (\x) numeric escapes, also maximal munch.
    assert (strByte "\o37" 1 == 31) "octal \\o37 == 31"
    assert (strByte "\xff" 1 == 255) "hex \\xff == 255"
    assert (strByte "\x41" 1 == 65) "hex \\x41 == 'A'"

    -- \& : the zero-width empty escape. Its only purpose is to terminate
    -- maximal munch. "\137\&0" is the two bytes 137 and '0' (48), NOT \1370.
    assert (strLen "\137\&0" == 2) "\\& separates: two bytes"
    assert (strByte "\137\&0" 1 == 137) "\\& separates: first byte 137"
    assert (strByte "\137\&0" 2 == 48) "\\& separates: second byte '0'"
    assert (strLen "a\&b" == 2) "\\& contributes nothing"

    -- Named ASCII control escapes, with maximal munch over the name table.
    assert (strByte "\NUL" 1 == 0) "\\NUL == 0"
    assert (strByte "\SOH" 1 == 1) "\\SOH == 1"
    assert (strByte "\US" 1 == 31) "\\US == 31"
    assert (strByte "\SP" 1 == 32) "\\SP == 32 (space)"
    assert (strByte "\DEL" 1 == 127) "\\DEL == 127"
    assert (strByte "\ESC" 1 == 27) "\\ESC == 27"
    -- \SOH wins over \SO + 'H' (longest name match); \& forces the shorter one.
    assert (strLen "\SOH" == 1) "\\SOH is one char (maximal munch)"
    assert (strLen "\SO\&H" == 2) "\\SO\\&H is two chars"
    assert (strByte "\SO\&H" 1 == 14) "\\SO\\&H first byte is SO (14)"
    assert (strByte "\SO\&H" 2 == 72) "\\SO\\&H second byte is 'H' (72)"

    -- String gap: backslash, whitespace (newlines allowed), backslash — the
    -- whole run produces nothing.
    assert (strLen "ab\  \cd" == 4) "string gap on one line joins"
    assert (strLen "hello \
                   \world" == 11) "multi-line string gap joins"

    -- Round-trip parity with the show side (read . show == id for the byte
    -- escapes). show emits these exact byte-escape spellings for bytes
    -- outside the printable ASCII range.
    assert (show "\181" == "\"\\181\"") "show \"\\181\" round-trips"
    assert (show "\0" == "\"\\NUL\"") "show \"\\0\" is \\NUL"
    assert (show "\SOH" == "\"\\SOH\"") "show \"\\SOH\" round-trips"
    assert (show "\DEL" == "\"\\DEL\"") "show \"\\DEL\" round-trips"
    -- The \& disambiguation the writer emits is exactly what the reader needs:
    -- the bytes [181,'5'] show as "\181\&5" (without \& the 5 would extend the
    -- number), and reading that back gives the same two bytes.
    assert (strByte "\181\&5" 1 == 181) "\\181\\&5 first byte 181"
    assert (strByte "\181\&5" 2 == 53) "\\181\\&5 second byte '5'"
    assert (show "\181\&5" == "\"\\181\\&5\"") "show inserts \\& before a digit"

    putStrLn "string escapes OK"
