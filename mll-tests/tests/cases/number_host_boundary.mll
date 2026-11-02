-- Every host->Number boundary delivers a double: an FFI result (bare, in
-- a tuple, in a LuaDict record's field, under LuaTry), a JSON number, a
-- Number map key read back through the builtins, a specialized body and a
-- dictionary-routed one alike. On Lua 5.3+ the host hands over the integer
-- subtype (`string.len`, `tonumber "5"`, an `os.date` table field), which
-- used to leak into Number arithmetic and wrap at 2^63; a doubles-only host
-- (LuaJIT) must print the same. Not GHC-twinnable (FFI, HashMap builtins,
-- the JSON library), so this case asserts its own output on both hosts.
import JSON
import qualified Data.Map as M

slen :: String -> LuaPure "string.len" Number
toNum :: String -> LuaPure "tonumber" Number
toNumT :: String -> LuaTry "tonumber" (Either String Number)
modfN :: Number -> LuaPure "math.modf" (Number, Number)
dateAt :: String -> Int -> LuaPure "os.date" Epoch

data Epoch = Epoch { year :: Number, month :: Number, hour :: Number }
    deriving (Show, LuaDict)

data Box = forall k. (Hashable k, Show k) => Box k

keysOf :: Box -> [String]
keysOf (Box k) = map show (hmKeys (hmInsert k True hmEmpty))

big :: Number
big = 4611686018427387904

check :: String -> String -> IO ()
check actual expected = do
    putStrLn actual
    assert (actual == expected) ("expected " <> expected <> ", got " <> actual)

main :: IO ()
main = do
    check (show (slen "abc", slen "abc" * big)) "(3.0,1.3835058055282164e19)"
    check (show (toNum "5", negate (toNum "0"), toNum "9007199254740993")) "(5.0,-0.0,9.007199254740992e15)"
    r7 <- toNumT "7"
    rx <- toNumT "x"
    check (show (r7, rx)) "(Right 7.0,Left \"unknown error\")"
    check (show (modfN 3.0)) "(3.0,0.0)"
    let e = dateAt "!*t" 0
    check (show (year e, month e * big, hour e)) "(1970.0,4.611686018427388e18,0.0)"
    check (show e) "Epoch {year = 1970.0, month = 1.0, hour = 0.0}"
    check (show (keysOf (Box (3 :: Number)), keysOf (Box (3 :: Int)))) "([\"3.0\"],[\"3\"])"
    let hm = hmFromList [(2, "a"), (big, "b")] :: HashMap Number String
    check (show (hmKeys hm)) "[2.0,4.611686018427388e18]"
    check (show (map (\k -> k + k) (hmKeys (hmInsert big () (hmEmpty :: HashMap Number ()))))) "[9.223372036854776e18]"
    check (show (hmToList (hmDelete 2 hm))) "[(4.611686018427388e18,\"b\")]"
    check (show hm) "{2.0 -> \"a\", 4.611686018427388e18 -> \"b\"}"
    check (case hmLookup 2 hm of { Just v -> v; Nothing -> "none" }) "a"
    check (show (M.keys (M.fromList [(toNum "3", ())]))) "[3.0]"
    case parseJSON "[3, 2.5, 9007199254740993, 1e2, -0]" of
        Right (JArr [JNum a, JNum b, JInt c, JNum d, JNum z]) ->
            check (show (a, a * big, b, c, d, negate z)) "(3.0,1.3835058055282164e19,2.5,9007199254740993,100.0,-0.0)"
        _ -> check "unexpected parse" "a JArr of five numbers"
    check (encodeJSON (JArr [toJSON (3 :: Int), toJSON (18014398509481984 :: Int), toJSON (2.0 :: Number), toJSON (0.5 :: Number)])) "[3,18014398509481984,2,0.5]"
    check (show (fromJSONInt (JNum 3.0), fromJSONInt (JInt 18014398509481984))) "(Right 3,Right 18014398509481984)"
    check (show (read "-0" :: Number, read "7" :: Number)) "(-0.0,7.0)"
