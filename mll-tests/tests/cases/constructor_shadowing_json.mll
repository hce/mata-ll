-- Derived JSON codecs on a type whose constructors shadow Prelude names:
-- the wire format must use the *source* names ("Ok"/"Err") while the
-- generated encoder/decoder internally constructs and matches the local
-- (shadowing) constructors — and the decoder's Either plumbing keeps using
-- the Prelude's own Left/Right. `Err` is declared FIRST (tag 1) so its tag
-- differs from ExitValue's Err (tag 2): with the old split-brain tag tables
-- this ordering miscompiled.
import JSON

data Status = Err String | Ok Integer deriving (ToJSON, FromJSON, Show, Eq)

main :: IO ()
main = do
    assert (encodeToJSON (Ok 3) == "{\"tag\":\"Ok\",\"contents\":3}") "encode Ok"
    assert (encodeToJSON (Err "nope") == "{\"tag\":\"Err\",\"contents\":\"nope\"}") "encode Err"
    case (decodeJSON "{\"tag\":\"Err\",\"contents\":\"bad\"}" :: Either String Status) of
        Left e -> error e
        Right v -> assert (v == Err "bad") "decode Err"
    case (decodeJSON "{\"tag\":\"Ok\",\"contents\":9}" :: Either String Status) of
        Left e -> error e
        Right v -> assert (v == Ok 9) "decode Ok"
    putStrLn "ok"
