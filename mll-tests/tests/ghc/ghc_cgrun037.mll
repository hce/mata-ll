-- GHC cgrun037: Map lookup and association lists
-- Tests using list of pairs as a lookup table

lookup_ :: String -> [(String, Int)] -> Maybe Int
lookup_ _ [] = Nothing
lookup_ key ((k, v):rest)
    | key == k  = Just v
    | otherwise = lookup_ key rest

insert_ :: String -> Int -> [(String, Int)] -> [(String, Int)]
insert_ k v [] = [(k, v)]
insert_ k v ((k2, v2):rest)
    | k == k2   = (k, v) : rest
    | otherwise  = (k2, v2) : insert_ k v rest

delete_ :: String -> [(String, Int)] -> [(String, Int)]
delete_ _ [] = []
delete_ k ((k2, v2):rest)
    | k == k2   = rest
    | otherwise  = (k2, v2) : delete_ k rest

main :: IO ()
main = do
    let table = [("one", 1), ("two", 2), ("three", 3)]
    assert (lookup_ "one" table == Just 1) "lookup one"
    assert (lookup_ "two" table == Just 2) "lookup two"
    assert (lookup_ "four" table == Nothing) "lookup miss"

    let table2 = insert_ "four" 4 table
    assert (lookup_ "four" table2 == Just 4) "insert new"

    let table3 = insert_ "two" 22 table
    assert (lookup_ "two" table3 == Just 22) "insert overwrite"

    let table4 = delete_ "two" table
    assert (lookup_ "two" table4 == Nothing) "delete"
    assert (lookup_ "one" table4 == Just 1) "delete keeps others"

    putStrLn "ok"
