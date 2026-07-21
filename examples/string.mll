-- Lua FFI: binding a host library function into typed mata-ll.
--
-- mata-ll runs as a guest inside a Lua host, so calling host functions is a
-- first-class feature, not an escape hatch. A signature whose result is
-- `LuaIterator "f" [E]` binds directly to the Lua stateful iterator `f`, each
-- step yielding an `E`; here `f` is Lua's `string.gmatch`. The result is always
-- written as an explicit list. A tuple element type decodes the pattern's
-- capture groups positionally — no glue code, the marshalling is generated
-- from the type.

gmatch :: String -> String -> LuaIterator "string.gmatch" [String]
gmatchPairs :: String -> String -> LuaIterator "string.gmatch" [(String, String)]

main :: IO ()
main = do
    -- Simple word splitting
    let words = gmatch "Mata lai le kiao" "%w+"
    mapM_ (\w -> putStrLn w) words

    -- Capture groups as tuples
    let pairs = gmatchPairs "name=Hans lang=MLL" "(%w+)=(%w+)"
    mapM_ (\p -> putStrLn (fst p <> " -> " <> snd p)) pairs
