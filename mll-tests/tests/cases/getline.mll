-- getLine: GHC-parity console input from the auto-Prelude — NO import.
-- GHC has getLine :: IO String in the Prelude; so does mata-ll. It reads one
-- line WITHOUT the trailing newline, and at end of input it raises a clean,
-- catchable error ("Prelude.getLine: end of input") — mata-ll's analog of
-- GHC's getLine throwing an isEOFError exception. Crucially it must NOT be
-- the raw Lua "attempt to concatenate a nil value" crash that a naked
-- io.read binding would produce.
--
-- The case drives a REAL end-of-file: it writes a two-line fixture file and
-- redirects Lua's default input to it with io.input (declared here as plain
-- in-case FFI — the point of the test is that getLine itself needs no
-- import). The harness gives every case a fresh Lua state, so the redirect
-- cannot leak into other tests.

newtype WriteHandle = WriteHandle LuaUserData

tmpName :: LuaIO "os.tmpname" String
fopen :: String -> String -> LuaTry "io.open" (Either String WriteHandle)
hWrite :: WriteHandle -> String -> LuaIO ":write" ()
hClose :: WriteHandle -> LuaIO ":close" ()
setInput :: String -> LuaIO "io.input" ()
removeFile :: String -> LuaIO "os.remove" ()

-- Plain-text substring search (string.find with plain=True): Right when
-- found, Left when not. Used to assert on the caught error message, which
-- carries a Lua source-position prefix before our text.
sfind :: String -> String -> Int -> Bool -> LuaTry "string.find" (Either String Int)

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

    -- getLine types as IO String, threads through do-notation, and strips
    -- the trailing newline (with it, l1 would be "alpha\n" and both the
    -- equality and the concatenation below would fail).
    l1 <- getLine
    assert (l1 == "alpha") "getLine returns the line without the trailing newline"
    l2 <- getLine
    assert (l2 == "beta") "getLine threads through do-notation"
    assert ((l1 <> "-" <> l2) == "alpha-beta") "getLine result works with string ops"

    -- At EOF getLine raises a clean error that try captures as Left.
    r3 <- try getLine
    case r3 of
        Right _  -> error "getLine at end of input should raise"
        Left msg -> do
            ok <- containsStr msg "Prelude.getLine: end of input"
            assert ok "EOF raises the Prelude.getLine: end of input error"
            bad <- containsStr msg "concatenate"
            assert (not bad) "EOF error is not the raw nil-concatenation Lua crash"

    -- catch also captures it, and the handler receives the message.
    v <- catch getLine (\e -> pure e)
    ok2 <- containsStr v "Prelude.getLine: end of input"
    assert ok2 "catch handler receives the EOF error message"

    removeFile path
    putStrLn "ok"
