-- readLine: LIO console input, hardened at end of input the same way as the
-- Prelude's getLine. readLine :: IO String requires `import LIO`, reads one
-- line WITHOUT the trailing newline, and at end of input raises the clean,
-- catchable error "LIO.readLine: end of input" — NOT the raw Lua "attempt to
-- concatenate a nil value" crash that the old naked io.read binding produced
-- when its EOF nil escaped into a String.
--
-- The case drives a REAL end-of-file: it writes a two-line fixture file and
-- redirects Lua's default input to it with io.input (declared here as plain
-- in-case FFI). The harness gives every case a fresh Lua state, so the
-- redirect cannot leak into other tests.

import LIO (readLine)

newtype WriteHandle = WriteHandle LuaUserData

tmpName :: LuaIO "os.tmpname" String
fopen :: String -> String -> LuaTry "io.open" WriteHandle
hWrite :: WriteHandle -> String -> LuaIO ":write" ()
hClose :: WriteHandle -> LuaIO ":close" ()
setInput :: String -> LuaIO "io.input" ()
removeFile :: String -> LuaIO "os.remove" ()

-- Plain-text substring search (string.find with plain=True): Right when
-- found, Left when not. Used to assert on the caught error message, which
-- carries a Lua source-position prefix before our text.
sfind :: String -> String -> Integer -> Bool -> LuaTry "string.find" Integer

containsStr :: String -> String -> IO Bool
containsStr s sub = do
    r <- sfind s sub 1 True
    case r of
        Left _  -> pure False
        Right _ -> pure True

main :: IO ()
main = do
    path <- tmpName
    r <- fopen path "w"
    case r of
        Left err -> error err
        Right h -> do
            hWrite h "alpha\nbeta\n"
            hClose h
    setInput path

    -- readLine types as IO String, threads through do-notation, and strips
    -- the trailing newline (with it, l1 would be "alpha\n" and both the
    -- equality and the concatenation below would fail).
    l1 <- readLine
    assert (l1 == "alpha") "readLine returns the line without the trailing newline"
    l2 <- readLine
    assert (l2 == "beta") "readLine threads through do-notation"
    assert ((l1 <> "-" <> l2) == "alpha-beta") "readLine result works with string ops"

    -- At EOF readLine raises a clean error that try captures as Left.
    r3 <- try readLine
    case r3 of
        Right _  -> error "readLine at end of input should raise"
        Left msg -> do
            ok <- containsStr msg "LIO.readLine: end of input"
            assert ok "EOF raises the LIO.readLine: end of input error"
            bad <- containsStr msg "concatenate"
            assert (not bad) "EOF error is not the raw nil-concatenation Lua crash"

    -- catch also captures it, and the handler receives the message.
    v <- catch readLine (\e -> pure e)
    ok2 <- containsStr v "LIO.readLine: end of input"
    assert ok2 "catch handler receives the EOF error message"

    removeFile path
    putStrLn "ok"
