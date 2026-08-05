import JSON
import Data.Generics

-- Integer fields through the native derived codecs, mixed with other types.
data Account = Account { owner :: String, balance :: Integer, note :: Maybe Integer }
    deriving (Eq, ToJSON, FromJSON)

-- The generic codecs resolve the ToJSON/FromJSON Integer instances at the
-- K1 leaves; they must agree with the native derives byte for byte.
data Wrapped = Wrapped { wval :: Integer }
    deriving (Eq, Generic, ToJSON, FromJSON)

big :: Integer
big = 2 ^ 100

decInteger :: String -> Either String Integer
decInteger s = decodeJSON s

decInt :: String -> Either String Int
decInt s = decodeJSON s

decAccount :: String -> Either String Account
decAccount s = decodeJSON s

okIs :: Eq a => Either String a -> a -> Bool
okIs (Right a) b = a == b
okIs (Left _) _ = False

errIs :: Either String a -> String -> Bool
errIs (Left e) m = e == m
errIs (Right _) _ = False

-- Parsed text re-encodes to the same bytes.
rtJson :: String -> Bool
rtJson s = case parseJSON s of
    Left _ -> False
    Right j -> encodeJSON j == s

parsesToJInt :: String -> Integer -> Bool
parsesToJInt s i = case parseJSON s of
    Right (JInt v) -> v == i
    _ -> False

parsesToJNum :: String -> Number -> Bool
parsesToJNum s n = case parseJSON s of
    Right (JNum v) -> v == n
    _ -> False

main :: IO ()
main = do
    -- Encoding: bare decimal digits at any magnitude, exactly like aeson.
    assert (encodeToJSON big == "1267650600228229401496703205376") "encode 2^100"
    assert (encodeToJSON (0 - big) == "-1267650600228229401496703205376") "encode -2^100"
    assert (encodeToJSON (5 :: Integer) == "5") "encode small Integer"
    assert (encodeToJSON (0 - 5 :: Integer) == "-5") "encode small negative Integer"
    assert (encodeToJSON (9223372036854775807 :: Integer) == "9223372036854775807") "encode int64 max as Integer"
    assert (encodeToJSON (0 - 9223372036854775808 :: Integer) == "-9223372036854775808") "encode int64 min as Integer"
    assert (encodeToJSON (9223372036854775808 :: Integer) == "9223372036854775808") "encode int64 max + 1"

    -- Decoding: exact at any magnitude from integer syntax.
    assert (okIs (decInteger "1267650600228229401496703205376") big) "decode 2^100"
    assert (okIs (decInteger "-1267650600228229401496703205376") (0 - big)) "decode -2^100"
    assert (okIs (decInteger "5") 5) "decode small Integer"
    assert (okIs (decInteger "-9223372036854775808") (0 - 9223372036854775808)) "decode int64 min"
    assert (okIs (decInteger "9223372036854775808") (9223372036854775807 + 1)) "decode int64 max + 1"
    -- Integral float syntax decodes like Int; fractions are rejected.
    assert (okIs (decInteger "1e2") 100) "decode exponent form"
    assert (errIs (decInteger "3.5") "expected an integer, but found the non-integral number 3.5") "reject fraction"
    assert (errIs (decInteger "\"5\"") "expected an integer, but found a string") "reject string"

    -- Int stays bounded: a big integer is rejected with the exact message.
    assert (errIs (decInt "1267650600228229401496703205376") "the number 1267650600228229401496703205376 is outside the 64-bit Int range") "Int rejects 2^100"
    assert (okIs (decInt "9223372036854775807") 9223372036854775807) "Int still takes int64 max"

    -- The parsed representation: big integer syntax is an exact JInt and
    -- its text round-trips byte-identically; in-window values stay JNum.
    assert (parsesToJInt "1267650600228229401496703205376" big) "parse to JInt"
    assert (parsesToJNum "42" 42.0) "parse to JNum"
    assert (rtJson "1267650600228229401496703205376") "text round-trip 2^100"
    assert (rtJson "-9223372036854775808") "text round-trip int64 min"
    assert (jInteger (JInt big) == Just big) "jInteger big"
    assert (jInteger (toJSON (7 :: Int)) == Just 7) "jInteger small"

    -- Derived record codecs.
    let acct = Account { owner = "hc", balance = big, note = Nothing }
    assert (encodeToJSON acct == "{\"owner\":\"hc\",\"balance\":1267650600228229401496703205376,\"note\":null}") "encode record"
    assert (okIs (decAccount (encodeToJSON acct)) acct) "record round-trip"
    assert (okIs (decAccount "{\"owner\":\"hc\",\"balance\":12}") (Account { owner = "hc", balance = 12, note = Nothing })) "optional field absent"

    -- Generic codecs agree with the native derive.
    let w = Wrapped { wval = big }
    assert (genericToJSON w == toJSON w) "generic encoder agrees"
    assert (encodeJSON (genericToJSON w) == "{\"wval\":1267650600228229401496703205376}") "generic encode bytes"
    assert (okIs (genericFromJSON (genericToJSON w)) w) "generic round-trip"

    putStrLn "ok"
