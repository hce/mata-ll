-- Regression: ReexJson derives ToJSON/FromJSON behind an export list
-- that does NOT re-export the JSON module, and this root never imports
-- JSON. The derived codecs must work anyway — they resolve in the
-- defining module's scope, not the root's. Before the fix, the derive
-- rejected with "the ToJSON class and its encoder combinators are not
-- in scope" the moment an export-list module was imported by a
-- JSON-less root.

import ReexJson

check :: (Eq a) => Either String a -> a -> String -> IO ()
check (Right got) want label = assert (got == want) label
check (Left err)  _    label = error (label <> ": " <> err)

main :: IO ()
main = do
    let p = Payload "x" 3 ["a", "b"]
    check (roundTrip p) p "gated record codec round-trips"
    check (shapeTrip (Circle 7)) (Circle 7) "gated tagged codec, payload"
    check (shapeTrip Point) Point "gated tagged codec, nullary"
