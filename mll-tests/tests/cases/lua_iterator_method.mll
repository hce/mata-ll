-- Method-form LuaIterator (audit finding 10): the iterator factory is a
-- METHOD on the first argument (`LuaIterator ":gmatch" …`). This used to
-- emit `__mll_iter(:gmatch, …)` — a Lua syntax error, so the module would
-- not even load. The receiver must be bound once and the method passed as
-- the factory with the receiver as its first argument.
gm :: String -> String -> LuaIterator ":gmatch" [String]

check :: (Show a, Eq a) => String -> a -> a -> IO ()
check name got want =
    if got == want
        then putStrLn ("ok " <> name)
        else error ("FAIL " <> name <> ": got " <> show got <> " want " <> show want)

main :: IO ()
main = do
    -- The words come out in order and fully decoded.
    check "words" (gm "one two three" "%a+") ["one", "two", "three"]
    -- Laziness across the method-form factory still holds: take 2 of a
    -- longer stream stops after two elements.
    check "take" (take 2 (gm "a b c d e" "%a+")) ["a", "b"]
    -- A computed (non-literal) receiver works: the receiver expression is
    -- evaluated once, inside the wrapper.
    let s = "x1" <> "y2" <> "z3"
    check "computed-receiver" (gm s "%d") ["1", "2", "3"]
