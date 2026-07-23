-- Regression test: error must force its message argument. The mata-ll name
-- `error` previously lowered to Lua's bare `error`, which received an
-- unforced thunk for any computed message and raised "table: 0x..." instead
-- of the text. (A string literal happened to work because it isn't thunked.)
-- The fix maps `error` to the forcing wrapper error_.

import LString (strLen, strSub)

boom :: Int -> Int
boom n = error ("bad value: " <> show n)

-- True if `s` ends with `suffix` (the caught message may carry a Lua location
-- prefix, so we check the tail rather than the whole string).
endsWith :: String -> String -> Bool
endsWith suffix s = strSub s (strLen s - strLen suffix + 1) (strLen s) == suffix

main :: IO ()
main = do
    -- A computed (thunked) message.
    v <- catch (seq (boom 5) (pure (0 :: Int)))
               (\msg -> if endsWith "bad value: 5" msg then pure 1 else pure 2)
    assert (v == 1) "computed error message is forced, not a thunk"

    -- A literal message still works.
    w <- catch (seq (error "plain" :: Int) (pure (0 :: Int)))
               (\msg -> if endsWith "plain" msg then pure 1 else pure 2)
    assert (w == 1) "literal error message still works"

    putStrLn "error_forces_message ok"
