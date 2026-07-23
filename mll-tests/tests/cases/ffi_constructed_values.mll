-- FFI marshalling with CONSTRUCTED values: every value crossing the boundary
-- here is built by computation — ranges, map/filter, `<>` concatenation,
-- JSON decoding, Maybe from Data.List's find — never written as a literal at
-- the call site. Literals hide marshalling bugs: a literal String is already
-- a native Lua string, a literal list compiles to an eager chain — it was a
-- CONSTRUCTED String (JSON-decoded) that regressed at c3cf855 while every
-- literal-based test stayed green. The Lua stdlib hosts used here are
-- argument-sensitive, so a regression fails loudly: table.concat raises on a
-- cons-cell table or an unforced element, string.upper on a table, and
-- string.rep's separator must be a native string (or genuinely absent).

import JSON
import Data.List (find)

data Cfg = Cfg { cfgTags :: [String], cfgSep :: String }
    deriving (Eq, FromJSON)

tblconcat  :: [String]  -> String -> LuaPure "table.concat" String
concatInts :: [Int] -> String -> LuaPure "table.concat" String
upper'     :: String    -> LuaPure "string.upper" String
rep'       :: String -> Number -> Maybe String -> LuaPure ":rep" String
mn         :: Number -> Maybe Number -> LuaPure "math.min" Number

cfg :: Cfg
cfg = case decodeJSON "{\"cfgTags\": [\"alpha\", \"beta\", \"gamma\"], \"cfgSep\": \"+\"}" of
        Right c -> c
        Left e  -> error e

main :: IO ()
main = do
    -- [String] built by mapping show over a range: each element is a thunk
    -- over a computed Int, not a native string until forced.
    let shown = map show [1..4]
    assert (tblconcat shown "," == "1,2,3,4")
        "map-show-built [String] crosses as a plain array of native strings"

    -- [Int] built by filtering a range.
    let evens = filter (\n -> n `mod` 2 == 0) [1..10]
    assert (concatInts evens "-" == "2-4-6-8-10")
        "filter-built [Int] crosses as a plain array of forced numbers"

    -- JSON-decoded list and separator: the exact shape that regressed —
    -- decoded Strings are cons structures until marshalled.
    assert (tblconcat (cfgTags cfg) (cfgSep cfg) == "alpha+beta+gamma")
        "JSON-decoded [String] and String cross natively"

    -- String built by concatenating decoded pieces.
    let joined = cfgSep cfg <> head (cfgTags cfg) <> cfgSep cfg
    assert (upper' joined == "+ALPHA+")
        "concatenation-built String is a native string"

    -- Maybe built by computation, both outcomes: find hits (the computed Just
    -- crosses as its unwrapped native-string payload) and misses (the computed
    -- Nothing is genuinely omitted; string.rep with a present separator
    -- behaves differently from an absent one).
    let hit = find (\s -> s == cfgSep cfg) (cfgTags cfg ++ [cfgSep cfg])
    assert (rep' "ab" 3.0 hit == "ab+ab+ab")
        "computed Just crosses as the unwrapped native-string payload"
    let miss = find (\s -> s == "zzz") (cfgTags cfg)
    assert (rep' "ab" 2.0 miss == "abab")
        "computed Nothing behaves like a literal Nothing"

    -- The omission itself, probed hard: math.min raises "bad argument #2
    -- (number expected, got nil)" on an explicit nil, so a computed Nothing
    -- that were passed as nil instead of omitted fails loudly.
    let noBound = findN (\x -> x > 9000.0) (map (\x -> x * 2.0) [1.5, 3.5])
    assert (mn 5.0 noBound == 5.0)
        "computed Nothing (Number) is genuinely omitted, not passed as nil"
    let bound = findN (\x -> x < 4.0) (map (\x -> x * 2.0) [1.5, 3.5])
    assert (mn 5.0 bound == 3.0)
        "computed Just (Number) from a mapped list crosses unwrapped"

    putStrLn "ffi constructed-values marshalling ok"

-- A monomorphic find over Numbers (keeps the Maybe construction in this file
-- independent of Data.List's polymorphic find used above).
findN :: (Number -> Bool) -> [Number] -> Maybe Number
findN _ [] = Nothing
findN p (x:xs) = if p x then Just x else findN p xs
