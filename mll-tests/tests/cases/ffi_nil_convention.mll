-- The nil convention of the FFI boundary: which mata-ll values are Lua's nil
-- on the host side, and which never are. Inside mata-ll nil represents the
-- empty list, Nothing and (), and only the declared type tells them apart;
-- at the boundary an empty list is the empty array {} in every position, an
-- empty map the empty table {}, and nil is the host form of exactly Nothing,
-- AnyNull and (). A HashMap value that was an empty list used to leave as
-- nil, which took its key with it: the host saw no entry, and the map that
-- came back had one entry fewer. And a host array with a nil between two
-- values was read to its largest index for a Maybe element only: an interior
-- AnyNull ended the list.
-- One line per cell; the label names the direction and position, then the
-- arm and the shape:
--   out/arg       the host form of a call's argument, written by the host
--                 (`form` renders a value as {key=value,...} with sorted
--                 keys and <tagged> for anything carrying a metatable)
--   out/optional  an optional positional argument: count of arguments:form
--   out/callback  the host form of what a mata-ll callback returns
--   in/result     a host value (given as Lua source) received as a result
--   in/callback   a host value received as a mata-ll callback's argument
--   in/constant   a host value read by name; in/iterator, one per step
--   echo          out as an argument, back as the result, unchanged by the
--                 host: the identity wherever the label does not say `lost`
-- The `lost` cells pin what a Lua table cannot carry: a value whose host
-- form is nil has no slot as a trailing list element, as a map value or as
-- a Maybe payload. The host is a chunk compiled with the standard `load`;
-- its functions are called as methods of the table it returns. Not
-- GHC-twinnable (FFI declarations), so the case asserts its own output on
-- both hosts.

newtype Host = Host LuaUserData

data Rec = Rec
    { rItems as "items" :: [Int]
    , rJust as "just" :: Maybe [Int]
    , rNone as "none" :: Maybe [Int]
    } deriving (Show, LuaDict)

data All = All
    { aList as "list" :: [Int]
    , aNested as "nested" :: [[Int]]
    , aJust as "just" :: Maybe [Int]
    , aNone as "none" :: Maybe [Int]
    , aPair as "pair" :: (String, [Int])
    , aRec as "rec" :: Rec
    , aMap as "map" :: HashMap String [Int]
    , aMaps as "maps" :: HashMap String (HashMap String Int)
    , aRows as "rows" :: [HashMap Int [[Int]]]
    } deriving (LuaDict)

chunk :: String -> LuaPure "load" LuaUserData
boot :: LuaUserData -> LuaPure "pcall" (Bool, Host)
hasText :: String -> String -> Int -> Bool -> LuaPure "string.find" (Maybe Int)

formList :: Host -> [Int] -> LuaPure ":form" String
formNested :: Host -> [[Int]] -> LuaPure ":form" String
formPair :: Host -> (String, [Int]) -> LuaPure ":form" String
formMaybes :: Host -> (Maybe [Int], Maybe [Int]) -> LuaPure ":form" String
formRec :: Host -> Rec -> LuaPure ":form" String
formMap :: Host -> HashMap String [Int] -> LuaPure ":form" String
formDeep :: Host -> HashMap Int [[Int]] -> LuaPure ":form" String
formMapMaybe :: Host -> HashMap String (Maybe [Int]) -> LuaPure ":form" String
formMaps :: Host -> HashMap String (HashMap String Int) -> LuaPure ":form" String
formEmptyMap :: Host -> HashMap String Int -> LuaPure ":form" String
formRows :: Host -> [HashMap String [Int]] -> LuaPure ":form" String
formAll :: Host -> All -> LuaPure ":form" String
formHoles :: Host -> [Maybe Int] -> LuaPure ":form" String
formAnys :: Host -> [Any] -> LuaPure ":form" String
formUnits :: Host -> [()] -> LuaPure ":form" String
formUnitMap :: Host -> HashMap String () -> LuaPure ":form" String
formNestedMaybe :: Host -> (Maybe (Maybe Int), Maybe (Maybe Int)) -> LuaPure ":form" String

argsOptional :: Host -> Maybe [Int] -> LuaPure ":args" String
argsUnit :: Host -> () -> LuaPure ":args" String

callbackList :: Host -> (Int -> [Int]) -> Int -> LuaPure ":callForm" String
callbackMaybe :: Host -> (Int -> Maybe [Int]) -> Int -> LuaPure ":callForm" String
callbackMap :: Host -> (Int -> HashMap String [Int]) -> Int -> LuaPure ":callForm" String
callbackAll :: Host -> (Int -> All) -> Int -> LuaPure ":callForm" String

makeList :: Host -> String -> LuaPure ":make" [Int]
makeNested :: Host -> String -> LuaPure ":make" [[Int]]
makePair :: Host -> String -> LuaPure ":make" [(String, [Int])]
makeMaybe :: Host -> String -> LuaPure ":make" (Maybe [Int])
makeRec :: Host -> String -> LuaPure ":make" Rec
makeMap :: Host -> String -> LuaPure ":make" (HashMap String [Int])
makeMaps :: Host -> String -> LuaPure ":make" (HashMap String (HashMap String Int))
makeAll :: Host -> String -> LuaPure ":make" All
makeAnys :: Host -> String -> LuaPure ":make" [Any]
makeHoles :: Host -> String -> LuaPure ":make" [Maybe Int]
makeUnits :: Host -> String -> LuaPure ":make" [()]
makeMapIO :: Host -> String -> LuaIO ":make" (HashMap String [Int])
makeMapCatch :: Host -> String -> LuaCatch ":make" (Either String (HashMap String [Int]))
makeMapTry :: Host -> String -> LuaTry ":make" (Either String (HashMap String [Int]))

constList :: LuaPure "package.loaded.mll_ffi_nil_convention.list" [Int]
constMaybe :: LuaPure "package.loaded.mll_ffi_nil_convention.list" (Maybe [Int])
constMap :: LuaPure "package.loaded.mll_ffi_nil_convention.map" (HashMap String [Int])
stepLists :: Host -> String -> LuaIterator ":steps" [[Int]]
stepMaps :: Host -> String -> LuaIterator ":steps" [HashMap String [Int]]

withList :: Host -> ([Int] -> String) -> String -> LuaPure ":callWith" String
withMap :: Host -> (HashMap String [Int] -> String) -> String -> LuaPure ":callWith" String
withAll :: Host -> (All -> String) -> String -> LuaPure ":callWith" String

echoList :: Host -> [Int] -> LuaPure ":echo" [Int]
echoNested :: Host -> [[Int]] -> LuaPure ":echo" [[Int]]
echoPairs :: Host -> [(String, [Int])] -> LuaPure ":echo" [(String, [Int])]
echoMaybes :: Host -> [(Maybe [Int], Maybe [Int])] -> LuaPure ":echo" [(Maybe [Int], Maybe [Int])]
echoRec :: Host -> Rec -> LuaPure ":echo" Rec
echoMap :: Host -> HashMap String [Int] -> LuaPure ":echo" (HashMap String [Int])
echoDeep :: Host -> HashMap Int [[Int]] -> LuaPure ":echo" (HashMap Int [[Int]])
echoMapMaybe :: Host -> HashMap String (Maybe [Int]) -> LuaPure ":echo" (HashMap String (Maybe [Int]))
echoMaps :: Host -> HashMap String (HashMap String Int) -> LuaPure ":echo" (HashMap String (HashMap String Int))
echoEmptyMap :: Host -> HashMap String Int -> LuaPure ":echo" (HashMap String Int)
echoAll :: Host -> All -> LuaPure ":echo" All
echoHoles :: Host -> [Maybe Int] -> LuaPure ":echo" [Maybe Int]
echoAnys :: Host -> [Any] -> LuaPure ":echo" [Any]
echoUnits :: Host -> [()] -> LuaPure ":echo" [()]
echoUnitMap :: Host -> HashMap String () -> LuaPure ":echo" (HashMap String ())
echoNestedMaybe :: Host -> [(Maybe (Maybe Int), Maybe (Maybe Int))] -> LuaPure ":echo" [(Maybe (Maybe Int), Maybe (Maybe Int))]
echoOptional :: Host -> Maybe [Int] -> LuaPure ":echo" (Maybe [Int])
echoMapIO :: Host -> HashMap String [Int] -> LuaIO ":echo" (HashMap String [Int])

hostSource :: String
hostSource = mconcat
    [ "local function d(x) "
    , "if type(x) ~= 'table' then return tostring(x) end "
    , "if getmetatable(x) ~= nil then return '<tagged>' end "
    , "local ks = {} for k in pairs(x) do ks[#ks + 1] = k end "
    , "table.sort(ks, function(a, b) return tostring(a) < tostring(b) end) "
    , "local ps = {} for i = 1, #ks do ps[i] = tostring(ks[i]) .. '=' .. d(x[ks[i]]) end "
    , "return '{' .. table.concat(ps, ',') .. '}' end "
    , "local ld = loadstring or load "
    , "local H = {} "
    , "function H:form(v) return d(v) end "
    , "function H:args(...) return select('#', ...) .. ':' .. d((...)) end "
    , "function H:make(src) return ld('return ' .. src)() end "
    , "function H:echo(v) return v end "
    , "function H:callForm(f, n) return d(f(n)) end "
    , "function H:callWith(f, src) return f(ld('return ' .. src)()) end "
    , "function H:steps(src) local items, i = ld('return ' .. src)(), 0 "
    , "return function() i = i + 1 return items[i] end end "
    , "package.loaded.mll_ffi_nil_convention = {list = {}, map = {empty = {}, full = {1, 1}}} "
    , "return H"
    ]

everything :: Int -> All
everything n = All
    { aList = []
    , aNested = [[], [n], []]
    , aJust = Just []
    , aNone = Nothing
    , aPair = ("x", [])
    , aRec = Rec [] (Just []) Nothing
    , aMap = hmFromList [("empty", []), ("full", [n, n])]
    , aMaps = hmFromList [("empty", hmFromList []), ("full", hmFromList [("k", n)])]
    , aRows = [hmFromList [(1, [[]]), (2, [])], hmFromList []]
    }

everythingForm :: String
everythingForm = mconcat
    [ "{just={},list={}"
    , ",map={empty={},full={1=1,2=1}}"
    , ",maps={empty={},full={k=1}}"
    , ",nested={1={},2={1=1},3={}}"
    , ",pair={1=x,2={}}"
    , ",rec={items={},just={}}"
    , ",rows={1={1={1={}},2={}},2={}}}"
    ]

everythingSource :: String
everythingSource = mconcat
    [ "{just={},list={}"
    , ",map={empty={},full={1,1}}"
    , ",maps={empty={},full={k=1}}"
    , ",nested={{},{1},{}}"
    , ",pair={'x',{}}"
    , ",rec={items={},just={}}"
    , ",rows={{{{}},{}},{}}}"
    ]

everythingShown :: String
everythingShown = mconcat
    [ "[]|[[],[1],[]]|Just []|Nothing|(\"x\",[])"
    , "|Rec {rItems = [], rJust = Just [], rNone = Nothing}"
    , "|[(\"empty\",[]),(\"full\",[1,1])]"
    , "|[(\"empty\",[]),(\"full\",[(\"k\",1)])]"
    , "|[[(1,[[]]),(2,[])],[]]"
    ]

sparse :: String
sparse = "(function() local t = {} t[1] = 1 t[3] = 3 return t end)()"

showAll :: All -> String
showAll a = mconcat
    [ show (aList a), "|", show (aNested a), "|", show (aJust a), "|", show (aNone a), "|", show (aPair a)
    , "|", show (aRec a)
    , "|", showMap (aMap a)
    , "|", show (map (\(k, inner) -> (k, hmToList inner)) (hmToList (aMaps a)))
    , "|", show (map hmToList (aRows a))
    ]

showMap :: HashMap String [Int] -> String
showMap m = show (hmToList m)

showAny :: Any -> String
showAny (AnyString s) = "s:" <> s
showAny (AnyInt i) = "i:" <> show i
showAny (AnyNumber x) = "n:" <> show x
showAny (AnyBool b) = "b:" <> show b
showAny AnyNull = "null"

right :: Either String a -> a
right (Right x) = x
right (Left m) = error ("unexpected Left: " <> m)

mentions :: String -> String -> Either String a -> String
mentions needle found (Left m) = case hasText m needle 1 True of
    Just _ -> found
    Nothing -> m
mentions _ _ (Right _) = "unexpected Right"

check :: String -> String -> String -> IO ()
check label actual expected = do
    putStrLn (label <> ": " <> actual)
    assert (actual == expected) (label <> ": expected " <> expected <> ", got " <> actual)

main :: IO ()
main = do
    let host = snd (boot (chunk hostSource))
    let one = length [host]
    let twoEntries = hmFromList [("empty", []), ("full", [one, one])]

    check "out/arg top-level: []" (formList host []) "{}"
    check "out/arg top-level: [1]" (formList host [one]) "{1=1}"
    check "out/arg list element: [[]]" (formNested host [[]]) "{1={}}"
    check "out/arg list element: [[],[1],[]]" (formNested host [[], [one], []]) "{1={},2={1=1},3={}}"
    check "out/arg tuple slot: (x,[])" (formPair host ("x", [])) "{1=x,2={}}"
    check "out/arg Maybe payload: (Just [],Nothing)" (formMaybes host (Just [], Nothing)) "{1={}}"
    check "out/arg Maybe payload: (Nothing,Just [])" (formMaybes host (Nothing, Just [])) "{2={}}"
    check "out/arg record field: [] / Just [] / Nothing" (formRec host (Rec [] (Just []) Nothing)) "{items={},just={}}"
    check "out/arg map value: []" (formMap host twoEntries) "{empty={},full={1=1,2=1}}"
    check "out/arg map value: only []" (formMap host (hmFromList [("empty", [])])) "{empty={}}"
    check "out/arg map value: a computed []" (formMap host (hmFromList [("none", filter (\k -> k > one) [one])])) "{none={}}"
    check "out/arg map value: [[]] and [] under Int keys" (formDeep host (hmFromList [(1, [[]]), (2, [])])) "{1={1={}},2={}}"
    check "out/arg map value: Just [] present, Nothing lost" (formMapMaybe host (hmFromList [("just", Just []), ("none", Nothing)])) "{just={}}"
    check "out/arg map value: empty map" (formMaps host (hmFromList [("empty", hmFromList []), ("full", hmFromList [("k", one)])])) "{empty={},full={k=1}}"
    check "out/arg top-level: empty map" (formEmptyMap host (hmFromList [])) "{}"
    check "out/arg list element: maps" (formRows host [hmFromList [("empty", [])], hmFromList []]) "{1={empty={}},2={}}"
    check "out/arg every arm at once" (formAll host (everything one)) everythingForm
    check "out/arg nil form: interior and trailing Nothing" (formHoles host [Just one, Nothing, Just 3, Nothing]) "{1=1,3=3}"
    check "out/arg nil form: interior and trailing AnyNull" (formAnys host [AnyInt one, AnyNull, AnyString "s", AnyNull]) "{1=1,3=s}"
    check "out/arg nil form: [(),()]" (formUnits host [(), ()]) "{}"
    check "out/arg nil form: map of ()" (formUnitMap host (hmFromList [("a", ())])) "{}"
    check "out/arg nil form: (Just Nothing,Just (Just 1))" (formNestedMaybe host (Just Nothing, Just (Just one))) "{2=1}"
    check "out/arg nil form: ()" (argsUnit host ()) "1:nil"

    check "out/optional Just []" (argsOptional host (Just [])) "1:{}"
    check "out/optional Just [1]" (argsOptional host (Just [one])) "1:{1=1}"
    check "out/optional Nothing" (argsOptional host Nothing) "0:nil"

    check "out/callback []" (callbackList host (\n -> filter (\k -> k > n) [n]) one) "{}"
    check "out/callback Just []" (callbackMaybe host (\n -> if n > 0 then Just [] else Nothing) one) "{}"
    check "out/callback Nothing" (callbackMaybe host (\n -> if n > 0 then Just [] else Nothing) 0) "nil"
    check "out/callback map value: []" (callbackMap host (\n -> hmFromList [("empty", []), ("full", [n, n])]) one) "{empty={},full={1=1,2=1}}"
    check "out/callback every arm at once" (callbackAll host everything one) everythingForm

    check "in/result {} at [Int]" (show (makeList host "{}")) "[]"
    check "in/result nil at [Int]" (show (makeList host "nil")) "[]"
    check "in/result {{},{1},{}} at [[Int]]" (show (makeNested host "{{},{1},{}}")) "[[],[1],[]]"
    check "in/result tuple slot {} and absent" (show (makePair host "{{'x',{}},{'y'}}")) "[(\"x\",[]),(\"y\",[])]"
    check "in/result {} at Maybe [Int]" (show (makeMaybe host "{}")) "Just []"
    check "in/result nil at Maybe [Int]" (show (makeMaybe host "nil")) "Nothing"
    check "in/result record fields {} and absent" (show (makeRec host "{just={}}")) "Rec {rItems = [], rJust = Just [], rNone = Nothing}"
    check "in/result map value {}" (showMap (makeMap host "{empty={},full={1,1}}")) "[(\"empty\",[]),(\"full\",[1,1])]"
    check "in/result map value {} counted" (show (hmSize (makeMap host "{empty={},full={1,1}}"))) "2"
    check "in/result empty map" (show (hmSize (makeMap host "{}"))) "0"
    check "in/result empty inner map" (show (map (\(k, inner) -> (k, hmSize inner)) (hmToList (makeMaps host "{empty={},full={k=1}}")))) "[(\"empty\",0),(\"full\",1)]"
    check "in/result every arm at once" (showAll (makeAll host everythingSource)) everythingShown
    check "in/result interior nil at [Maybe Int]" (show (makeHoles host sparse)) "[Just 1,Nothing,Just 3]"
    check "in/result interior nil at [Any]" (show (map showAny (makeAnys host sparse))) "[\"i:1\",\"null\",\"i:3\"]"
    check "in/result interior nil at [()]" (show (length (makeUnits host sparse))) "3"
    viaIO <- makeMapIO host "{empty={}}"
    check "in/result map value {} under LuaIO" (showMap viaIO) "[(\"empty\",[])]"
    check "in/result map value {} under LuaCatch" (showMap (right (makeMapCatch host "{empty={}}"))) "[(\"empty\",[])]"
    viaTry <- makeMapTry host "{empty={}}"
    check "in/result map value {} under LuaTry" (showMap (right viaTry)) "[(\"empty\",[])]"
    nilMap <- try (putStrLn (showMap (makeMap host "nil")))
    check "in/result nil at a HashMap raises" (mentions "declared HashMap String [Int] but the host returned nil" "raised" nilMap) "raised"
    nilPair <- try (putStrLn (showAll (makeAll host "{rec={},map={},maps={}}")))
    check "in/result nil at a tuple raises" (mentions "declared (String, [Int]) but the host returned nil" "raised" nilPair) "raised"
    nilRec <- try (putStrLn (showAll (makeAll host "{pair={'x'},map={},maps={}}")))
    check "in/result nil at a record raises" (mentions "declared Rec but the host returned nil" "raised" nilRec) "raised"
    nilField <- try (putStrLn (showAll (makeAll host "{pair={'x'},rec={},maps={}}")))
    check "in/result nil at a HashMap field raises" (mentions "declared HashMap String [Int] but the host returned nil" "raised" nilField) "raised"
    check "in/result nil at every list field" (showAll (makeAll host "{pair={'x'},rec={},map={},maps={}}")) "[]|[]|Nothing|Nothing|(\"x\",[])|Rec {rItems = [], rJust = Nothing, rNone = Nothing}|[]|[]|[]"

    check "in/constant {} at [Int]" (show constList) "[]"
    check "in/constant {} at Maybe [Int]" (show constMaybe) "Just []"
    check "in/constant map value {}" (showMap constMap) "[(\"empty\",[]),(\"full\",[1,1])]"
    check "in/iterator step {}" (show (stepLists host "{{},{1},{}}")) "[[],[1],[]]"
    check "in/iterator step map value {}" (show (map hmToList (stepMaps host "{{empty={}},{}}"))) "[[(\"empty\",[])],[]]"

    check "in/callback {} at [Int]" (withList host show "{}") "[]"
    check "in/callback map value {}" (withMap host showMap "{empty={},full={1,1}}") "[(\"empty\",[]),(\"full\",[1,1])]"
    check "in/callback every arm at once" (withAll host showAll everythingSource) everythingShown

    check "echo []" (show (echoList host [])) "[]"
    check "echo [[],[1],[]]" (show (echoNested host [[], [one], []])) "[[],[1],[]]"
    check "echo [[]]" (show (echoNested host [[]])) "[[]]"
    check "echo tuple slot" (show (echoPairs host [("x", []), ("y", [one])])) "[(\"x\",[]),(\"y\",[1])]"
    check "echo Maybe payload" (show (echoMaybes host [(Just [], Nothing), (Nothing, Just [one])])) "[(Just [],Nothing),(Nothing,Just [1])]"
    check "echo record" (show (echoRec host (Rec [] (Just []) Nothing))) "Rec {rItems = [], rJust = Just [], rNone = Nothing}"
    check "echo map value []" (showMap (echoMap host twoEntries)) "[(\"empty\",[]),(\"full\",[1,1])]"
    check "echo map value [] keeps the size" (show (hmSize (echoMap host twoEntries))) "2"
    check "echo map value [] is found" (show (hmLookup "empty" (echoMap host twoEntries))) "Just []"
    check "echo map value [[]] and [] under Int keys" (show (hmToList (echoDeep host (hmFromList [(1, [[]]), (2, [])])))) "[(1,[[]]),(2,[])]"
    check "echo map of maps" (show (map (\(k, inner) -> (k, hmToList inner)) (hmToList (echoMaps host (hmFromList [("empty", hmFromList []), ("full", hmFromList [("k", one)])]))))) "[(\"empty\",[]),(\"full\",[(\"k\",1)])]"
    check "echo empty map" (show (hmSize (echoEmptyMap host (hmFromList [])))) "0"
    check "echo every arm at once" (showAll (echoAll host (everything one))) everythingShown
    viaEchoIO <- echoMapIO host twoEntries
    check "echo map value [] under LuaIO" (showMap viaEchoIO) "[(\"empty\",[]),(\"full\",[1,1])]"
    check "echo optional Just []" (show (echoOptional host (Just []))) "Just []"
    check "echo optional Nothing" (show (echoOptional host Nothing)) "Nothing"
    check "echo interior Nothing, trailing lost" (show (echoHoles host [Just one, Nothing, Just 3, Nothing])) "[Just 1,Nothing,Just 3]"
    check "echo interior AnyNull, trailing lost" (show (map showAny (echoAnys host [AnyInt one, AnyNull, AnyString "s", AnyNull]))) "[\"i:1\",\"null\",\"s:s\"]"
    check "echo [(),()] lost" (show (length (echoUnits host [(), ()]))) "0"
    check "echo map value () lost" (show (hmSize (echoUnitMap host (hmFromList [("a", ())])))) "0"
    check "echo map value Nothing lost" (show (hmToList (echoMapMaybe host (hmFromList [("just", Just []), ("none", Nothing)])))) "[(\"just\",Just [])]"
    check "echo Just Nothing lost" (show (echoNestedMaybe host [(Just Nothing, Just (Just one))])) "[(Nothing,Just (Just 1))]"
