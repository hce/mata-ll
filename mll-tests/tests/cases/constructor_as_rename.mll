-- Constructor `as "name"` renaming: a sum-type constructor may carry an
-- external name (`Con field-types as "name"`), the per-constructor twin of
-- the field-level `as "key"` rename. The external name is the constructor's
-- JSON TAG in a derived ToJSON/FromJSON codec: the bare string of a nullary
-- constructor and the "tag" value of a fielded one. It affects ONLY the
-- JSON boundary — Show keeps the source constructor name (Show is for
-- debugging, Haskell-style), and construction, pattern matching and the
-- runtime representation are unchanged. Asserted against exact encoded
-- strings so a symmetric encode/decode bug cannot hide behind a passing
-- round-trip, plus decodes of hand-written documents by the external tag.
import JSON
import LString

-- All-nullary sum, every constructor renamed: encodes to the bare external
-- strings.
data Suit = Clubs as "clubs" | Diamonds as "diamonds" | Hearts as "hearts" | Spades as "spades"
    deriving (Show, Eq, ToJSON, FromJSON)

-- Fielded constructors renamed: the external name is the "tag" value,
-- positional arguments stay under "contents".
data Outcome = Ok Integer as "ok" | Err String as "error"
    deriving (Show, Eq, ToJSON, FromJSON)

-- Mixed: renamed and unrenamed constructors in one type; the unrenamed ones
-- keep the source name as before.
data Ev = Warn String as "warning" | Fatal String | Silent
    deriving (Show, Eq, ToJSON, FromJSON)

-- A renamed RECORD constructor in a tagged sum: the external name is the
-- "tag" value, and a field-level rename applies inline next to it — the two
-- rename levels compose.
data Msg = Tick { mAt as "at" :: Integer } as "tick" | Stop as "stop"
    deriving (Show, Eq, ToJSON, FromJSON)

-- A lone nullary constructor is tagged by definition: the external name is
-- the whole payload.
data Unit2 = MkUnit2 as "unit"
    deriving (Show, Eq, ToJSON, FromJSON)

rtS :: Suit -> Bool
rtS x = case decodeJSON (encodeToJSON x) of
    Right y -> y == x
    Left _ -> False

rtO :: Outcome -> Bool
rtO x = case decodeJSON (encodeToJSON x) of
    Right y -> y == x
    Left _ -> False

rtE :: Ev -> Bool
rtE x = case decodeJSON (encodeToJSON x) of
    Right y -> y == x
    Left _ -> False

rtM :: Msg -> Bool
rtM x = case decodeJSON (encodeToJSON x) of
    Right y -> y == x
    Left _ -> False

rtU :: Unit2 -> Bool
rtU x = case decodeJSON (encodeToJSON x) of
    Right y -> y == x
    Left _ -> False

isLeft2 :: Either String a -> Bool
isLeft2 (Left _) = True
isLeft2 (Right _) = False

-- Decode helpers (one per type: Eq dispatch is monomorphic).
decS :: String -> Suit -> Bool
decS s x = case (decodeJSON s :: Either String Suit) of
    Right y -> y == x
    Left _ -> False

decO :: String -> Outcome -> Bool
decO s x = case (decodeJSON s :: Either String Outcome) of
    Right y -> y == x
    Left _ -> False

decE :: String -> Ev -> Bool
decE s x = case (decodeJSON s :: Either String Ev) of
    Right y -> y == x
    Left _ -> False

decM :: String -> Msg -> Bool
decM s x = case (decodeJSON s :: Either String Msg) of
    Right y -> y == x
    Left _ -> False

decU :: String -> Unit2 -> Bool
decU s x = case (decodeJSON s :: Either String Unit2) of
    Right y -> y == x
    Left _ -> False

main :: IO ()
main = do
    -- ============================================================
    -- Exact encoded strings use the EXTERNAL name
    -- ============================================================
    assert (encodeToJSON Spades == "\"spades\"") "nullary encodes to the external string"
    assert (encodeToJSON Clubs == "\"clubs\"") "first nullary constructor renamed too"
    assert (encodeToJSON (Err "x") == "{\"tag\":\"error\",\"contents\":\"x\"}") "fielded tag is the external name"
    assert (encodeToJSON (Ok 42) == "{\"tag\":\"ok\",\"contents\":42}") "single argument stays unwrapped under contents"
    assert (encodeToJSON (Tick { mAt = 7 }) == "{\"tag\":\"tick\",\"at\":7}") "record: renamed tag and renamed field compose"
    assert (encodeToJSON Stop == "\"stop\"") "nullary in a record sum encodes to the external string"
    assert (encodeToJSON MkUnit2 == "\"unit\"") "lone nullary constructor encodes to the external string"

    -- Mixed: unrenamed constructors keep the source name.
    assert (encodeToJSON (Warn "w") == "{\"tag\":\"warning\",\"contents\":\"w\"}") "renamed constructor in a mixed sum"
    assert (encodeToJSON (Fatal "f") == "{\"tag\":\"Fatal\",\"contents\":\"f\"}") "unrenamed fielded constructor keeps the source tag"
    assert (encodeToJSON Silent == "\"Silent\"") "unrenamed nullary constructor keeps the source name"

    -- ============================================================
    -- Show is UNCHANGED: the source constructor name
    -- ============================================================
    assert (show Spades == "Spades") "show keeps the source name (nullary)"
    assert (show (Ok 42) == "Ok 42") "show keeps the source name (fielded)"
    assert (show (Warn "w") == "Warn \"w\"" || show (Warn "w") == "Warn w") "show keeps the source name (mixed)"
    assert (show MkUnit2 == "MkUnit2") "show keeps the source name (lone nullary)"

    -- ============================================================
    -- Decoding dispatches on the EXTERNAL tag
    -- ============================================================
    assert (decS "\"spades\"" Spades) "bare external string decodes"
    assert (decS "{\"tag\":\"spades\"}" Spades) "tagged-object form of a renamed nullary decodes"
    assert (decO "{\"tag\":\"error\",\"contents\":\"boom\"}" (Err "boom")) "fielded external tag decodes"
    assert (decO "{\"tag\":\"ok\",\"contents\":3}" (Ok 3)) "second fielded external tag decodes"
    assert (decM "{\"tag\":\"tick\",\"at\":9}" (Tick { mAt = 9 })) "record external tag + renamed field decode"
    assert (decE "{\"tag\":\"Fatal\",\"contents\":\"f\"}" (Fatal "f")) "unrenamed tag still decodes in a mixed sum"
    assert (decU "\"unit\"" MkUnit2) "lone nullary external string decodes"

    -- The SOURCE name of a renamed constructor is no longer a valid tag:
    -- the codec has exactly one external spelling per constructor.
    assert (isLeft2 (decodeJSON "\"Spades\"" :: Either String Suit)) "source name of a renamed nullary is rejected"
    assert (isLeft2 (decodeJSON "{\"tag\":\"Err\",\"contents\":\"x\"}" :: Either String Outcome)) "source tag of a renamed fielded constructor is rejected"
    assert (isLeft2 (decodeJSON "{\"tag\":\"Warn\",\"contents\":\"w\"}" :: Either String Ev)) "source tag rejected in a mixed sum too"

    -- The unknown-tag error names the EXTERNAL tags — that is what a
    -- document must contain.
    case (decodeJSON "\"Bogus\"" :: Either String Suit) of
        Right _ -> assert False "bogus tag must not decode"
        Left e -> assert (strContains e "'spades'" && not (strContains e "'Spades'")) "unknown-tag message lists the external tags"

    -- ============================================================
    -- Round-trips
    -- ============================================================
    assert (rtS Clubs && rtS Diamonds && rtS Hearts && rtS Spades) "all renamed nullary constructors round-trip"
    assert (rtO (Ok 0) && rtO (Err "")) "renamed fielded constructors round-trip"
    assert (rtE (Warn "a") && rtE (Fatal "b") && rtE Silent) "mixed sum round-trips"
    assert (rtM (Tick { mAt = 1 }) && rtM Stop) "record sum with composed renames round-trips"
    assert (rtU MkUnit2) "lone renamed nullary round-trips"

    putStrLn "constructor as-rename ok"

strContains :: String -> String -> Bool
strContains h n = go 1
  where
    go i = if i + strLen n - 1 > strLen h
           then False
           else if strSub h i (i + strLen n - 1) == n
                then True
                else go (i + 1)
