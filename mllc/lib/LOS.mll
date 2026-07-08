-- MLL bindings for Lua 5.4 os library

-- Time
clock :: LuaIO "os.clock" Number
time :: LuaIO "os.time" Integer
difftime :: Integer -> Integer -> LuaPure "os.difftime" Number

-- Date formatting (os.date with format string)
date :: String -> LuaPure "os.date" String

-- Environment
getenv :: String -> LuaTry "os.getenv" String

-- File operations
remove :: String -> LuaTry "os.remove" ()
rename :: String -> String -> LuaTry "os.rename" ()

-- Process
execute :: String -> LuaIO "os.execute" Bool
exit :: Integer -> LuaIO "os.exit" ()
tmpname :: LuaIO "os.tmpname" String
