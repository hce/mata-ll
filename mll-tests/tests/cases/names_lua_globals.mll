import LString
import LMath

data Cfg = Cfg { width :: Int, height :: Int, select :: Int } deriving (LuaDict)

lengths :: Int -> Int -> Int
lengths string math = string * 100 + math * 10 + strLen "abc" + floor (sqrt 16.0)

chars :: Int -> String -> String
chars string acc = acc <> strChar (string + 64) <> strSub "xyz" string string

widen :: Int -> Cfg -> Cfg
widen pairs c = c { width = width c + pairs }

lookupOr :: Int -> HashMap String Int -> Int
lookupOr rawequal m = case hmLookup "k" m of
    Just v -> v + rawequal
    Nothing -> rawequal

sizes :: Int -> Int -> HashMap String Int -> Int
sizes hmSize hashmap_size m = hmSize * 100 + hashmap_size * 10 + length (hmKeys m)

bytes :: Int -> Int -> Int
bytes bsLength bsEmpty = bsLength * 10 + bsEmpty

cells :: Int -> Int -> IO Int
cells newIORef readIORef = pure (newIORef - readIORef)

announce :: Int -> Int -> IO Int
announce print tostring = do
    putStrLn ("announce " <> show tostring)
    pure (print + tostring)

picks :: Cfg -> Int -> Int
picks c next = select c + next

main :: IO ()
main = do
    k <- pure (2 :: Int)
    assert (lengths k 3 == 237) "locals named string and math next to string.len and math.sqrt"
    assert (chars k "" == "By") "a local named string next to string.char and string.sub"
    assert (width (widen k (Cfg 1 2 3)) == 3) "a local named pairs around a LuaDict record update"
    assert (lookupOr k (hmInsert "k" k hmEmpty) == 4) "a local named rawequal around a fused lookup"
    assert (lookupOr k hmEmpty == 2) "the lookup's miss"
    assert (sizes k 3 (hmInsert "a" 1 hmEmpty) == 231) "locals named hmSize and hashmap_size"
    assert (bytes k 3 == 23) "locals named like the ByteString builtins"
    r <- cells 9 k
    assert (r == 7) "locals named like the IORef intrinsics"
    a <- announce k 3
    assert (a == 5) "locals named print and tostring"
    assert (picks (Cfg 1 2 3) k == 5) "a LuaDict field named select"
    putStrLn "ok"
