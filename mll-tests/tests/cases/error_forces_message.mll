-- Regression test: error must force its message argument. The mata-ll name
-- `error` previously lowered to Lua's bare `error`, which received an
-- unforced thunk for any computed message and raised "table: 0x..." instead
-- of the text. (A string literal happened to work because it isn't thunked.)
-- The fix maps `error` to the forcing wrapper error_.

-- Since Q78 the caught message is EXACTLY the error text: error_ raises at
-- Lua level 0, so no "file:line:" position prefix is prepended (GHC's
-- `error "boom"` delivers "boom" to a catcher). The import keeps this case
-- outside the GHC-oracle domain (LString is Lua string FFI).
import LString (strLen)

boom :: Int -> Int
boom n = error ("bad value: " <> show n)

main :: IO ()
main = do
    -- A computed (thunked) message, received verbatim.
    v <- catch (seq (boom 5) (pure (0 :: Int)))
               (\msg -> if msg == "bad value: 5" then pure 1 else pure 2)
    assert (v == 1) "computed error message is forced and unprefixed"
    assert (strLen "x" == 1) "(keep the LString import live)"

    -- A literal message still works.
    w <- catch (seq (error "plain" :: Int) (pure (0 :: Int)))
               (\msg -> if msg == "plain" then pure 1 else pure 2)
    assert (w == 1) "literal error message still works"

    putStrLn "error_forces_message ok"
