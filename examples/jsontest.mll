import JSON

data MyStruct = MyStruct
        { msName        as "name"       :: String
        , msAge         as "age"        :: Integer
        , msFirstNames  as "firstNames" :: [String]
        } deriving (ToJSON, FromJSON, Show)

per1 :: MyStruct
per1 = MyStruct "E" 32 ["hc"]

per2 :: MyStruct
per2 = MyStruct "J" 64 ["W", "J"]

per3 :: MyStruct
per3 = MyStruct "D" 128 ["芸"]

main :: IO ()
main = mapM_ (putStrLn . encodeToJSON) [per1, per2, per3]
