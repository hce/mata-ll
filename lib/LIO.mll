-- LIO: Lua io library bindings

-- Opaque file handle (Lua userdata with metatable methods)
newtype FileHandle = FileHandle LuaUserData

-- Error convention: Lua functions return nil on failure
data IOResult a = IOSuccess a | IOFailure String

-- Default stream operations (stdin/stdout)
readLine :: LuaIO "io.read" String
readStdin :: String -> LuaIO "io.read" String
writeStdout :: String -> LuaIO "io.write" ()
flushStdout :: LuaIO "io.flush" ()

-- File open (returns Either String FileHandle: Left err | Right handle)
fOpen :: String -> String -> LuaTry "io.open" FileHandle
fClose :: FileHandle -> LuaIO ":close" ()

-- File methods (handle as first arg, colon-call in Lua)
fRead :: FileHandle -> String -> LuaIO ":read" String
fReadLine :: FileHandle -> LuaIO ":read" (Maybe String)
fWrite :: FileHandle -> String -> LuaIO ":write" ()
fFlush :: FileHandle -> LuaIO ":flush" ()
fSeek :: FileHandle -> String -> Integer -> LuaIO ":seek" Integer

-- Read all lines from a file handle (accumulator helper)
fReadAllLines :: FileHandle -> [String] -> IO [String]
fReadAllLines handle acc = do
    line <- fReadLine handle
    case line of
        Nothing -> pure (reverse acc)
        Just l  -> fReadAllLines handle (l : acc)

-- Read all lines from a file (eagerly, no lazy IO)
fileLines :: String -> IO [String]
fileLines path = do
    result <- fOpen path "r"
    case result of
        Left err -> error err
        Right handle -> do
            lines <- fReadAllLines handle []
            fClose handle
            pure lines
