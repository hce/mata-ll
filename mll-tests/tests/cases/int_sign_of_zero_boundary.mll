-- The host-boundary observers of an Int's sign of zero on a doubles-only
-- host (N9, M10): the HashMap builtins — a -0.0 Int key shares its slot
-- with 0 and reads back as 0, a structural key holding one shows as 0 —
-- and the JSON library's Int routes: `toJSON (negate 0 :: Int)` encodes
-- as 0, as aeson does (through the runtime's Int -> Number conversion),
-- in a list, under Maybe, in a derived encoder, and a decoded integral
-- number reads back as the Int 0. Not GHC-twinnable (HashMap builtins,
-- JSON FFI library), so this case asserts its own output and must print
-- the same on both hosts.
import JSON

data Q = Q { qx :: Int, qs :: [Int] }
    deriving (Show, Eq, ToJSON, FromJSON)

check :: String -> String -> IO ()
check actual expected = do
    putStrLn actual
    assert (actual == expected) ("expected " <> expected <> ", got " <> actual)

main :: IO ()
main = do
    let z = length ([] :: [Int])
        nz = negate z
        t = truncate (-0.5 :: Number) :: Int
    check (show (hmKeys (hmInsert nz "a" hmEmpty), hmToList (hmInsert 0 "b" (hmInsert nz "a" hmEmpty)))) "([0],[(0,\"b\")])"
    check (show (hmLookup 0 (hmInsert nz "a" hmEmpty), hmMember nz (hmInsert 0 "a" hmEmpty), hmSize (hmInsert nz True (hmInsert 0 True hmEmpty)))) "(Just \"a\",True,1)"
    check (show (hmKeys (hmInsert (nz, t) "a" hmEmpty), hmLookup (0, 0) (hmInsert (nz, t) "a" hmEmpty))) "([(0,0)],Just \"a\")"
    check (encodeJSON (toJSON nz)) "0"
    check (encodeJSON (toJSON [nz, 1, t])) "[0,1,0]"
    check (encodeJSON (toJSON (Just nz))) "0"
    check (encodeToJSON (Q { qx = nz, qs = [t, nz] })) "{\"qx\":0,\"qs\":[0,0]}"
    check (show (fromJSONInt (JNum (negate 0)), decodeJSON "-0" :: Either String Int, decodeJSON "{\"qx\":-0,\"qs\":[0]}" :: Either String Q)) "(Right 0,Right 0,Right (Q {qx = 0, qs = [0]}))"
