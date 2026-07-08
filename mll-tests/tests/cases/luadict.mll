-- LuaDict deriving: the constructor is laid out as a Lua table keyed by field
-- name (`{width = …}`) instead of a positional array, for interop with Lua
-- APIs that take dictionaries. Construction, field access, record update,
-- pattern matching and derived Show/Eq must all go through the named keys.

data Config = Config { width :: Integer, height :: Integer, title :: String }
  deriving (Show, Eq, LuaDict)

-- A LuaDict whose field names are Lua reserved words, to exercise the
-- bracketed-key path (`{["local"] = …}`, `t["end"]`) in construction, access
-- and update. These are valid mata-ll identifiers but not valid bare Lua keys.
data Span = Span { local :: Integer, end :: Integer }
  deriving (Eq, LuaDict)

area :: Config -> Integer
area c = c.width * c.height

-- Positional pattern binds each variable from its named key.
sumDims :: Config -> Integer
sumDims (Config w h _) = w + h

resize :: Config -> Integer -> Config
resize c w = c { width = w }

-- FFI: hand the LuaDict table straight to a Lua function that reads a named
-- key. This only works if the value reaches Lua as a real dictionary.
rawget :: Config -> String -> LuaPure "rawget" Integer

main :: IO ()
main = do
    let c = Config { width = 80, height = 25, title = "hi" }
    -- accessor function and dot syntax, both keyed by name
    assert (width c == 80) "accessor width"
    assert (c.height == 25) "dot height"
    assert (c.title == "hi") "dot title"
    assert (area c == 2000) "computed area"
    -- positional construction produces the same named table
    let p = Config 10 20 "pos"
    assert (p.width == 10) "positional construction width"
    assert (p.title == "pos") "positional construction title"
    -- pattern match binds by key
    assert (sumDims c == 105) "pattern match binds by key"
    -- record update copies the dict and overwrites one key
    let d = resize c 100
    assert (d.width == 100) "update changes width"
    assert (d.height == 25) "update keeps height"
    assert (d.title == "hi") "update keeps title"
    -- derived Eq and Show work over the named layout
    assert (c == c) "derived Eq reflexive"
    assert (not (c == d)) "derived Eq distinguishes"
    assert (show c == "Config 80 25 hi") "derived Show"
    -- reserved-word field keys round-trip through bracketed access/update
    let s = Span { local = 7, end = 3 }
    assert (s.local == 7) "reserved-word key access"
    assert (s.end == 3) "reserved-word key access 2"
    let s2 = s { local = 9 }
    assert (s2.local == 9) "reserved-word key update"
    assert (s2.end == 3) "reserved-word key update keeps other"
    -- interop: a Lua function reads the named keys directly
    assert (rawget c "width" == 80) "Lua reads named key"
    assert (rawget c "height" == 25) "Lua reads named key 2"
    putStrLn "luadict ok"
