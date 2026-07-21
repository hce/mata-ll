floor' :: Number -> LuaPure "math.floor" Number
ceil :: Number -> LuaPure "math.ceil" Number

-- LuaIterator: wraps a Lua iterator factory into a lazy MLL list
gmatch :: String -> String -> LuaIterator "string.gmatch" [String]

main :: IO ()
main = do
    assert (floor' 3.7 == 3.0) "floor 3.7"
    assert (ceil 3.2 == 4.0) "ceil 3.2"
    assert (sqrt 4.0 == 2.0) "sqrt 4"
    -- LuaIterator test: gmatch returns words as a lazy list
    let words = gmatch "hello world foo" "%w+"
    assert (head words == "hello") "gmatch head"
    assert (head (tail words) == "world") "gmatch second"
    assert (head (tail (tail words)) == "foo") "gmatch third"
    assert (length words == 3) "gmatch length"
