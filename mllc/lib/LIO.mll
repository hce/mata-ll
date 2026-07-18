-- LIO: Lua io library bindings

-- Opaque file handle (Lua userdata with metatable methods)
newtype FileHandle = FileHandle LuaUserData

-- Error convention: Lua functions return nil on failure
data IOResult a = IOSuccess a | IOFailure String

-- Default stream operations (stdin/stdout)
-- readLine reads one line from stdin WITHOUT the trailing newline (io.read's
-- default "l" format already strips it). ffi_readLine is the raw internal
-- binding: at end of input io.read() returns nil, which LuaTry surfaces as
-- Left rather than letting a nil String escape (and crash later with
-- "attempt to concatenate a nil value"). readLine turns that Left into the
-- clean, catchable error "LIO.readLine: end of input" — the same hardening
-- as the Prelude's getLine (catch it with try/catch).
ffi_readLine :: LuaTry "io.read" (Either String String)

readLine :: IO String
readLine = do
    r <- ffi_readLine
    case r of
        Left _  -> error "LIO.readLine: end of input"
        Right s -> pure s

readStdin :: String -> LuaIO "io.read" String
writeStdout :: String -> LuaIO "io.write" ()
flushStdout :: LuaIO "io.flush" ()

-- File open (returns Either String FileHandle: Left err | Right handle)
fOpen :: String -> String -> LuaTry "io.open" (Either String FileHandle)
fClose :: FileHandle -> LuaIO ":close" ()

-- File methods (handle as first arg, colon-call in Lua)
fRead :: FileHandle -> String -> LuaIO ":read" String

-- Read exactly n raw bytes from the current position (Lua's file:read(n)
-- numeric form). fRead covers the string formats ("l", "a", ...); this is
-- the binary-safe counterpart for reading a byte count, e.g. fixed-size
-- on-disk structures. At end of file Lua returns nil, which is an error
-- here — seek within bounds before reading.
fReadN :: FileHandle -> Integer -> LuaIO ":read" String
fReadLine :: FileHandle -> LuaIO ":read" (Maybe String)
fWrite :: FileHandle -> String -> LuaIO ":write" ()
fFlush :: FileHandle -> LuaIO ":flush" ()
fSeek :: FileHandle -> String -> Integer -> LuaIO ":seek" Integer

-- Read all lines from a file (eagerly, no lazy IO)
fileLines :: String -> IO [String]
fileLines path = do
    result <- fOpen path "r"
    case result of
        Left err -> error err
        Right handle -> do
            lines <- readAll handle []
            fClose handle
            pure lines
  where
    readAll handle acc = do
        line <- fReadLine handle
        case line of
            Nothing -> pure (reverse acc)
            Just l  -> readAll handle (l : acc)
