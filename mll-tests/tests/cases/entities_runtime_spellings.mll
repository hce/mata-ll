import JSON
import qualified EntWireA as A
import qualified EntWireB as B

data Status = Active | Retired
    deriving (Show, Eq, ToJSON, FromJSON)

data Item = Item { name :: Bool }
    deriving (Show, Eq, ToJSON, FromJSON)

rawgetStrA :: A.Cfg -> String -> LuaPure "rawget" String
rawgetIntA :: A.Cfg -> String -> LuaPure "rawget" Int
rawgetIntB :: B.Cfg -> String -> LuaPure "rawget" Int

main :: IO ()
main = do
    assert (A.encA A.Active == "\"Active\"") "A's constructor tag is its source name"
    assert (B.encB B.Paused == "\"Paused\"" && B.encB B.Active == "\"Active\"") "B's constructor tags"
    assert (encodeToJSON Retired == "\"Retired\"" && encodeToJSON Active == "\"Active\"") "the entry module's tags"
    assert (A.encItemA (A.Item "pen" 2) == "{\"name\":\"pen\",\"count\":2}") "A's field keys"
    assert (B.encItemB (B.Item 5 ["t"]) == "{\"name\":5,\"tags\":[\"t\"]}") "B's field keys"
    assert (encodeToJSON (Item True) == "{\"name\":true}") "the entry module's field key"
    assert (A.decA "{\"name\":\"ink\",\"count\":4}" == Right (A.Item "ink" 4)) "decoding by source keys"
    assert ((decodeJSON "\"Gone\"" :: Either String B.Status) == Right B.Gone) "decoding a tag of B"
    assert ((decodeJSON "{\"name\":false}" :: Either String Item) == Right (Item False)) "decoding the entry module's record"
    assert (A.metaA True == "Node.Node" && A.metaA False == "Node.Leaf") "Generic datatypeName and conName"
    assert (B.selB 3 == "label") "Generic selName"
    assert (rawgetStrA (A.cfgA 7) "title" == "a" && rawgetIntA (A.cfgA 7) "width" == 7) "A's LuaDict keys"
    assert (rawgetIntB (B.cfgB 9) "title" == 9 && rawgetIntB (B.cfgB 9) "width" == 1) "B's LuaDict keys"
    assert (show (A.Item "x" 1) == "Item {name = \"x\", count = 1}" && show B.Paused == "Paused") "derived Show"
    putStrLn "ok"
