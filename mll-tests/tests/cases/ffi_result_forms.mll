-- Every FFI call form decodes a host result the same way. One line per cell
-- of the table (call form x result shape); the label names the cell:
--   forms   P LuaPure, I LuaIO, C LuaCatch, IC LuaIOCatch, T LuaTry, each
--           also with a trailing M for the `:method` spelling of the same
--           host function; K a constant read; IT/ITM a LuaIterator step
--   shapes  int/str scalar, num Number (the host hands over an integer),
--           mbJ/mbN Maybe present/absent, list, tup/tupNum/tupNil/tup3 a
--           top-level tuple (the call's return values, counted by arity),
--           nest/mbTup a tuple nested in a list/Maybe (a positional table),
--           rec a LuaDict record, hm a HashMap, unit
-- A method-form `LuaPure ":m"` result was never decoded (`Just 3` arrived as
-- the bare 3), a method-form tuple result emitted Lua that did not load, and
-- a tuple under LuaTry/LuaCatch/LuaIOCatch was rejected because the wrapper
-- kept one return value. The host functions are the standard library's:
-- `rawget` reads a table built from mata-ll values, whose metatable makes
-- the same function callable as a method, `next`/`ipairs`/`string.byte`
-- return several values, `error`/`string.format` raise. Not GHC-twinnable
-- (FFI declarations), so the case asserts its own output on both hosts.

newtype Obj = Obj LuaUserData

data Rec = Rec
    { rName as "name" :: String
    , rPort as "port" :: Int
    , rTags as "tags" :: [String]
    , rAlias as "alias" :: Maybe String
    } deriving (LuaDict)

data View = View
    { vName as "name" :: String
    , vPort as "port" :: Number
    , vTags as "tags" :: [String]
    , vAlias as "alias" :: Maybe String
    } deriving (Show, LuaDict)

data Fixture = Fixture
    { fI as "i" :: Int
    , fS as "s" :: String
    , fXs as "xs" :: [Int]
    , fPr as "pr" :: (Int, String)
    , fPrs as "prs" :: [(Int, String)]
    , fRc as "rc" :: Rec
    , fHm as "hm" :: HashMap String Int
    } deriving (LuaDict)

globals :: LuaPure "_G" LuaUserData
loaded :: LuaPure "package.loaded" LuaUserData
wrap :: Fixture -> HashMap String LuaUserData -> LuaIO "setmetatable" Obj
wrapOne :: HashMap String Int -> HashMap String LuaUserData -> LuaIO "setmetatable" Obj
publish :: LuaUserData -> String -> Obj -> LuaIO "rawset" LuaUserData
hasText :: String -> String -> Int -> Bool -> LuaPure "string.find" (Maybe Int)

intP :: Obj -> String -> LuaPure "rawget" Int
intPM :: Obj -> String -> LuaPure ":rawget" Int
intI :: Obj -> String -> LuaIO "rawget" Int
intIM :: Obj -> String -> LuaIO ":rawget" Int
intC :: Obj -> String -> LuaCatch "rawget" (Either String Int)
intCM :: Obj -> String -> LuaCatch ":rawget" (Either String Int)
intIC :: Obj -> String -> LuaIOCatch "rawget" (Either String Int)
intICM :: Obj -> String -> LuaIOCatch ":rawget" (Either String Int)
intT :: Obj -> String -> LuaTry "rawget" (Either String Int)
intTM :: Obj -> String -> LuaTry ":rawget" (Either String Int)

strP :: Obj -> String -> LuaPure "rawget" String
strPM :: Obj -> String -> LuaPure ":rawget" String
strI :: Obj -> String -> LuaIO "rawget" String
strIM :: Obj -> String -> LuaIO ":rawget" String
strC :: Obj -> String -> LuaCatch "rawget" (Either String String)
strCM :: Obj -> String -> LuaCatch ":rawget" (Either String String)
strIC :: Obj -> String -> LuaIOCatch "rawget" (Either String String)
strICM :: Obj -> String -> LuaIOCatch ":rawget" (Either String String)
strT :: Obj -> String -> LuaTry "rawget" (Either String String)
strTM :: Obj -> String -> LuaTry ":rawget" (Either String String)

numP :: Obj -> String -> LuaPure "rawget" Number
numPM :: Obj -> String -> LuaPure ":rawget" Number
numI :: Obj -> String -> LuaIO "rawget" Number
numIM :: Obj -> String -> LuaIO ":rawget" Number
numC :: Obj -> String -> LuaCatch "rawget" (Either String Number)
numCM :: Obj -> String -> LuaCatch ":rawget" (Either String Number)
numIC :: Obj -> String -> LuaIOCatch "rawget" (Either String Number)
numICM :: Obj -> String -> LuaIOCatch ":rawget" (Either String Number)
numT :: Obj -> String -> LuaTry "rawget" (Either String Number)
numTM :: Obj -> String -> LuaTry ":rawget" (Either String Number)

mbJP :: Obj -> String -> LuaPure "rawget" (Maybe Int)
mbJPM :: Obj -> String -> LuaPure ":rawget" (Maybe Int)
mbJI :: Obj -> String -> LuaIO "rawget" (Maybe Int)
mbJIM :: Obj -> String -> LuaIO ":rawget" (Maybe Int)
mbJC :: Obj -> String -> LuaCatch "rawget" (Either String (Maybe Int))
mbJCM :: Obj -> String -> LuaCatch ":rawget" (Either String (Maybe Int))
mbJIC :: Obj -> String -> LuaIOCatch "rawget" (Either String (Maybe Int))
mbJICM :: Obj -> String -> LuaIOCatch ":rawget" (Either String (Maybe Int))
mbJT :: Obj -> String -> LuaTry "rawget" (Either String (Maybe Int))
mbJTM :: Obj -> String -> LuaTry ":rawget" (Either String (Maybe Int))

mbNP :: Obj -> String -> LuaPure "rawget" (Maybe Int)
mbNPM :: Obj -> String -> LuaPure ":rawget" (Maybe Int)
mbNI :: Obj -> String -> LuaIO "rawget" (Maybe Int)
mbNIM :: Obj -> String -> LuaIO ":rawget" (Maybe Int)
mbNC :: Obj -> String -> LuaCatch "rawget" (Either String (Maybe Int))
mbNCM :: Obj -> String -> LuaCatch ":rawget" (Either String (Maybe Int))
mbNIC :: Obj -> String -> LuaIOCatch "rawget" (Either String (Maybe Int))
mbNICM :: Obj -> String -> LuaIOCatch ":rawget" (Either String (Maybe Int))
mbNT :: Obj -> String -> LuaTry "rawget" (Either String (Maybe Int))
mbNTM :: Obj -> String -> LuaTry ":rawget" (Either String (Maybe Int))

listP :: Obj -> String -> LuaPure "rawget" [Int]
listPM :: Obj -> String -> LuaPure ":rawget" [Int]
listI :: Obj -> String -> LuaIO "rawget" [Int]
listIM :: Obj -> String -> LuaIO ":rawget" [Int]
listC :: Obj -> String -> LuaCatch "rawget" (Either String [Int])
listCM :: Obj -> String -> LuaCatch ":rawget" (Either String [Int])
listIC :: Obj -> String -> LuaIOCatch "rawget" (Either String [Int])
listICM :: Obj -> String -> LuaIOCatch ":rawget" (Either String [Int])
listT :: Obj -> String -> LuaTry "rawget" (Either String [Int])
listTM :: Obj -> String -> LuaTry ":rawget" (Either String [Int])

tupP :: Obj -> LuaPure "next" (String, Int)
tupPM :: Obj -> LuaPure ":next" (String, Int)
tupI :: Obj -> LuaIO "next" (String, Int)
tupIM :: Obj -> LuaIO ":next" (String, Int)
tupC :: Obj -> LuaCatch "next" (Either String (String, Int))
tupCM :: Obj -> LuaCatch ":next" (Either String (String, Int))
tupIC :: Obj -> LuaIOCatch "next" (Either String (String, Int))
tupICM :: Obj -> LuaIOCatch ":next" (Either String (String, Int))
tupT :: Obj -> LuaTry "next" (Either String (String, Int))
tupTM :: Obj -> LuaTry ":next" (Either String (String, Int))

tupNumP :: String -> Int -> Int -> LuaPure "string.byte" (Number, Number)
tupNumPM :: String -> Int -> Int -> LuaPure ":byte" (Number, Number)
tupNumI :: String -> Int -> Int -> LuaIO "string.byte" (Number, Number)
tupNumIM :: String -> Int -> Int -> LuaIO ":byte" (Number, Number)
tupNumC :: String -> Int -> Int -> LuaCatch "string.byte" (Either String (Number, Number))
tupNumCM :: String -> Int -> Int -> LuaCatch ":byte" (Either String (Number, Number))
tupNumIC :: String -> Int -> Int -> LuaIOCatch "string.byte" (Either String (Number, Number))
tupNumICM :: String -> Int -> Int -> LuaIOCatch ":byte" (Either String (Number, Number))
tupNumT :: String -> Int -> Int -> LuaTry "string.byte" (Either String (Number, Number))
tupNumTM :: String -> Int -> Int -> LuaTry ":byte" (Either String (Number, Number))

tupNilP :: Obj -> String -> LuaPure "next" (Maybe String, Maybe Int)
tupNilPM :: Obj -> String -> LuaPure ":next" (Maybe String, Maybe Int)
tupNilI :: Obj -> String -> LuaIO "next" (Maybe String, Maybe Int)
tupNilIM :: Obj -> String -> LuaIO ":next" (Maybe String, Maybe Int)
tupNilC :: Obj -> String -> LuaCatch "next" (Either String (Maybe String, Maybe Int))
tupNilCM :: Obj -> String -> LuaCatch ":next" (Either String (Maybe String, Maybe Int))
tupNilIC :: Obj -> String -> LuaIOCatch "next" (Either String (Maybe String, Maybe Int))
tupNilICM :: Obj -> String -> LuaIOCatch ":next" (Either String (Maybe String, Maybe Int))
tupNilT :: Obj -> String -> LuaTry "next" (Either String (Maybe String, Maybe Int))
tupNilTM :: Obj -> String -> LuaTry ":next" (Either String (Maybe String, Maybe Int))

tup3P :: Obj -> LuaPure "ipairs" (LuaUserData, LuaUserData, Int)
tup3PM :: Obj -> LuaPure ":ipairs" (LuaUserData, LuaUserData, Int)
tup3I :: Obj -> LuaIO "ipairs" (LuaUserData, LuaUserData, Int)
tup3IM :: Obj -> LuaIO ":ipairs" (LuaUserData, LuaUserData, Int)
tup3C :: Obj -> LuaCatch "ipairs" (Either String (LuaUserData, LuaUserData, Int))
tup3CM :: Obj -> LuaCatch ":ipairs" (Either String (LuaUserData, LuaUserData, Int))
tup3IC :: Obj -> LuaIOCatch "ipairs" (Either String (LuaUserData, LuaUserData, Int))
tup3ICM :: Obj -> LuaIOCatch ":ipairs" (Either String (LuaUserData, LuaUserData, Int))
tup3T :: Obj -> LuaTry "ipairs" (Either String (LuaUserData, LuaUserData, Int))
tup3TM :: Obj -> LuaTry ":ipairs" (Either String (LuaUserData, LuaUserData, Int))

nestP :: Obj -> String -> LuaPure "rawget" [(Int, String)]
nestPM :: Obj -> String -> LuaPure ":rawget" [(Int, String)]
nestI :: Obj -> String -> LuaIO "rawget" [(Int, String)]
nestIM :: Obj -> String -> LuaIO ":rawget" [(Int, String)]
nestC :: Obj -> String -> LuaCatch "rawget" (Either String [(Int, String)])
nestCM :: Obj -> String -> LuaCatch ":rawget" (Either String [(Int, String)])
nestIC :: Obj -> String -> LuaIOCatch "rawget" (Either String [(Int, String)])
nestICM :: Obj -> String -> LuaIOCatch ":rawget" (Either String [(Int, String)])
nestT :: Obj -> String -> LuaTry "rawget" (Either String [(Int, String)])
nestTM :: Obj -> String -> LuaTry ":rawget" (Either String [(Int, String)])

mbTupP :: Obj -> String -> LuaPure "rawget" (Maybe (Int, String))
mbTupPM :: Obj -> String -> LuaPure ":rawget" (Maybe (Int, String))
mbTupI :: Obj -> String -> LuaIO "rawget" (Maybe (Int, String))
mbTupIM :: Obj -> String -> LuaIO ":rawget" (Maybe (Int, String))
mbTupC :: Obj -> String -> LuaCatch "rawget" (Either String (Maybe (Int, String)))
mbTupCM :: Obj -> String -> LuaCatch ":rawget" (Either String (Maybe (Int, String)))
mbTupIC :: Obj -> String -> LuaIOCatch "rawget" (Either String (Maybe (Int, String)))
mbTupICM :: Obj -> String -> LuaIOCatch ":rawget" (Either String (Maybe (Int, String)))
mbTupT :: Obj -> String -> LuaTry "rawget" (Either String (Maybe (Int, String)))
mbTupTM :: Obj -> String -> LuaTry ":rawget" (Either String (Maybe (Int, String)))

recP :: Obj -> String -> LuaPure "rawget" View
recPM :: Obj -> String -> LuaPure ":rawget" View
recI :: Obj -> String -> LuaIO "rawget" View
recIM :: Obj -> String -> LuaIO ":rawget" View
recC :: Obj -> String -> LuaCatch "rawget" (Either String View)
recCM :: Obj -> String -> LuaCatch ":rawget" (Either String View)
recIC :: Obj -> String -> LuaIOCatch "rawget" (Either String View)
recICM :: Obj -> String -> LuaIOCatch ":rawget" (Either String View)
recT :: Obj -> String -> LuaTry "rawget" (Either String View)
recTM :: Obj -> String -> LuaTry ":rawget" (Either String View)

hmP :: Obj -> String -> LuaPure "rawget" (HashMap String Int)
hmPM :: Obj -> String -> LuaPure ":rawget" (HashMap String Int)
hmI :: Obj -> String -> LuaIO "rawget" (HashMap String Int)
hmIM :: Obj -> String -> LuaIO ":rawget" (HashMap String Int)
hmC :: Obj -> String -> LuaCatch "rawget" (Either String (HashMap String Int))
hmCM :: Obj -> String -> LuaCatch ":rawget" (Either String (HashMap String Int))
hmIC :: Obj -> String -> LuaIOCatch "rawget" (Either String (HashMap String Int))
hmICM :: Obj -> String -> LuaIOCatch ":rawget" (Either String (HashMap String Int))
hmT :: Obj -> String -> LuaTry "rawget" (Either String (HashMap String Int))
hmTM :: Obj -> String -> LuaTry ":rawget" (Either String (HashMap String Int))

unitP :: Obj -> String -> LuaPure "rawget" ()
unitPM :: Obj -> String -> LuaPure ":rawget" ()
unitI :: Obj -> String -> LuaIO "rawget" ()
unitIM :: Obj -> String -> LuaIO ":rawget" ()
unitC :: Obj -> String -> LuaCatch "rawget" (Either String ())
unitCM :: Obj -> String -> LuaCatch ":rawget" (Either String ())
unitIC :: Obj -> String -> LuaIOCatch "rawget" (Either String ())
unitICM :: Obj -> String -> LuaIOCatch ":rawget" (Either String ())
unitT :: Obj -> String -> LuaTry "rawget" (Either String ())
unitTM :: Obj -> String -> LuaTry ":rawget" (Either String ())

intK :: LuaPure "package.loaded.mll_ffi_result_forms.i" Int
strK :: LuaPure "package.loaded.mll_ffi_result_forms.s" String
numK :: LuaPure "package.loaded.mll_ffi_result_forms.i" Number
mbJK :: LuaPure "package.loaded.mll_ffi_result_forms.i" (Maybe Int)
mbNK :: LuaPure "package.loaded.mll_ffi_result_forms.absent" (Maybe Int)
listK :: LuaPure "package.loaded.mll_ffi_result_forms.xs" [Int]
tupK :: LuaPure "package.loaded.mll_ffi_result_forms.pr" (Int, String)
nestK :: LuaPure "package.loaded.mll_ffi_result_forms.prs" [(Int, String)]
mbTupK :: LuaPure "package.loaded.mll_ffi_result_forms.pr" (Maybe (Int, String))
recK :: LuaPure "package.loaded.mll_ffi_result_forms.rc" View
hmK :: LuaPure "package.loaded.mll_ffi_result_forms.hm" (HashMap String Int)
unitK :: LuaPure "package.loaded.mll_ffi_result_forms.absent" ()

strIT :: String -> String -> LuaIterator "string.gmatch" [String]
strITM :: String -> String -> LuaIterator ":gmatch" [String]
intIT :: String -> String -> LuaIterator "string.gmatch" [Int]
intITM :: String -> String -> LuaIterator ":gmatch" [Int]
numIT :: String -> String -> LuaIterator "string.gmatch" [Number]
numITM :: String -> String -> LuaIterator ":gmatch" [Number]
tupIT :: String -> String -> LuaIterator "string.gmatch" [(String, Int)]
tupITM :: String -> String -> LuaIterator ":gmatch" [(String, Int)]
tupNumIT :: String -> String -> LuaIterator "string.gmatch" [(Number, Number)]
tupNumITM :: String -> String -> LuaIterator ":gmatch" [(Number, Number)]
firstIT :: String -> String -> LuaIterator "string.gmatch" [String]
firstITM :: String -> String -> LuaIterator ":gmatch" [String]

raiseC :: String -> Int -> LuaCatch "error" (Either String (Int, String))
raiseIC :: String -> Int -> LuaIOCatch "error" (Either String [Int])
formatCM :: String -> String -> LuaCatch ":format" (Either String (Maybe String))
formatICM :: String -> String -> LuaIOCatch ":format" (Either String (String, Int))
selectT :: Int -> Maybe Int -> String -> LuaTry "select" (Either String (Int, String))
rawgetTM :: Obj -> String -> LuaTry ":rawget" (Either String [Int])
shortP :: Obj -> String -> LuaPure "next" (String, Int)
shortCM :: Obj -> String -> LuaCatch ":next" (Either String (String, Int))

big :: Number
big = 4611686018427387904

numShow :: Number -> String
numShow x = show (x, x * big)

numsShow :: [Number] -> String
numsShow xs = show (xs, head xs * big)

hmShow :: HashMap String Int -> String
hmShow m = show (hmSize m, hmLookup "b" m)

third :: (LuaUserData, LuaUserData, Int) -> String
third (_, _, n) = show n

right :: Either String a -> a
right (Right x) = x
right (Left m) = error ("unexpected Left: " <> m)

leftMsg :: Either String a -> String
leftMsg (Left m) = m
leftMsg (Right _) = "unexpected Right"

mentions :: String -> Either String a -> String
mentions needle (Left m) = case hasText m needle 1 True of
    Just _ -> needle
    Nothing -> m
mentions _ (Right _) = "unexpected Right"

check :: String -> String -> String -> IO ()
check label actual expected = do
    putStrLn (label <> ": " <> actual)
    assert (actual == expected) (label <> ": expected " <> expected <> ", got " <> actual)

checkIO :: String -> IO a -> (a -> String) -> String -> IO ()
checkIO label action render expected = do
    value <- action
    check label (render value) expected

main :: IO ()
main = do
    let meta = hmFromList [("__index", globals)]
    obj <- wrap (Fixture 3 "ca" [1, 2, 3] (1, "a") [(1, "a"), (2, "b")] (Rec "ca" 443 ["x", "y"] Nothing) (hmFromList [("a", 1), ("b", 2)])) meta
    one <- wrapOne (hmFromList [("only", 42)]) meta
    _ <- publish loaded "mll_ffi_result_forms" obj
    check "P/int" (show (intP obj "i")) "3"
    check "PM/int" (show (intPM obj "i")) "3"
    checkIO "I/int" (intI obj "i") show "3"
    checkIO "IM/int" (intIM obj "i") show "3"
    check "C/int" (show (right (intC obj "i"))) "3"
    check "CM/int" (show (right (intCM obj "i"))) "3"
    checkIO "IC/int" (intIC obj "i") (show . right) "3"
    checkIO "ICM/int" (intICM obj "i") (show . right) "3"
    checkIO "T/int" (intT obj "i") (show . right) "3"
    checkIO "TM/int" (intTM obj "i") (show . right) "3"
    check "P/str" (show (strP obj "s")) "\"ca\""
    check "PM/str" (show (strPM obj "s")) "\"ca\""
    checkIO "I/str" (strI obj "s") show "\"ca\""
    checkIO "IM/str" (strIM obj "s") show "\"ca\""
    check "C/str" (show (right (strC obj "s"))) "\"ca\""
    check "CM/str" (show (right (strCM obj "s"))) "\"ca\""
    checkIO "IC/str" (strIC obj "s") (show . right) "\"ca\""
    checkIO "ICM/str" (strICM obj "s") (show . right) "\"ca\""
    checkIO "T/str" (strT obj "s") (show . right) "\"ca\""
    checkIO "TM/str" (strTM obj "s") (show . right) "\"ca\""
    check "P/num" (numShow (numP obj "i")) "(3.0,1.3835058055282164e19)"
    check "PM/num" (numShow (numPM obj "i")) "(3.0,1.3835058055282164e19)"
    checkIO "I/num" (numI obj "i") numShow "(3.0,1.3835058055282164e19)"
    checkIO "IM/num" (numIM obj "i") numShow "(3.0,1.3835058055282164e19)"
    check "C/num" (numShow (right (numC obj "i"))) "(3.0,1.3835058055282164e19)"
    check "CM/num" (numShow (right (numCM obj "i"))) "(3.0,1.3835058055282164e19)"
    checkIO "IC/num" (numIC obj "i") (numShow . right) "(3.0,1.3835058055282164e19)"
    checkIO "ICM/num" (numICM obj "i") (numShow . right) "(3.0,1.3835058055282164e19)"
    checkIO "T/num" (numT obj "i") (numShow . right) "(3.0,1.3835058055282164e19)"
    checkIO "TM/num" (numTM obj "i") (numShow . right) "(3.0,1.3835058055282164e19)"
    check "P/mbJ" (show (mbJP obj "i")) "Just 3"
    check "PM/mbJ" (show (mbJPM obj "i")) "Just 3"
    checkIO "I/mbJ" (mbJI obj "i") show "Just 3"
    checkIO "IM/mbJ" (mbJIM obj "i") show "Just 3"
    check "C/mbJ" (show (right (mbJC obj "i"))) "Just 3"
    check "CM/mbJ" (show (right (mbJCM obj "i"))) "Just 3"
    checkIO "IC/mbJ" (mbJIC obj "i") (show . right) "Just 3"
    checkIO "ICM/mbJ" (mbJICM obj "i") (show . right) "Just 3"
    checkIO "T/mbJ" (mbJT obj "i") (show . right) "Just 3"
    checkIO "TM/mbJ" (mbJTM obj "i") (show . right) "Just 3"
    check "P/mbN" (show (mbNP obj "absent")) "Nothing"
    check "PM/mbN" (show (mbNPM obj "absent")) "Nothing"
    checkIO "I/mbN" (mbNI obj "absent") show "Nothing"
    checkIO "IM/mbN" (mbNIM obj "absent") show "Nothing"
    check "C/mbN" (show (right (mbNC obj "absent"))) "Nothing"
    check "CM/mbN" (show (right (mbNCM obj "absent"))) "Nothing"
    checkIO "IC/mbN" (mbNIC obj "absent") (show . right) "Nothing"
    checkIO "ICM/mbN" (mbNICM obj "absent") (show . right) "Nothing"
    checkIO "T/mbN" (mbNT obj "absent") leftMsg "unknown error"
    checkIO "TM/mbN" (mbNTM obj "absent") leftMsg "unknown error"
    check "P/list" (show (listP obj "xs")) "[1,2,3]"
    check "PM/list" (show (listPM obj "xs")) "[1,2,3]"
    checkIO "I/list" (listI obj "xs") show "[1,2,3]"
    checkIO "IM/list" (listIM obj "xs") show "[1,2,3]"
    check "C/list" (show (right (listC obj "xs"))) "[1,2,3]"
    check "CM/list" (show (right (listCM obj "xs"))) "[1,2,3]"
    checkIO "IC/list" (listIC obj "xs") (show . right) "[1,2,3]"
    checkIO "ICM/list" (listICM obj "xs") (show . right) "[1,2,3]"
    checkIO "T/list" (listT obj "xs") (show . right) "[1,2,3]"
    checkIO "TM/list" (listTM obj "xs") (show . right) "[1,2,3]"
    check "P/tup" (show (tupP one)) "(\"only\",42)"
    check "PM/tup" (show (tupPM one)) "(\"only\",42)"
    checkIO "I/tup" (tupI one) show "(\"only\",42)"
    checkIO "IM/tup" (tupIM one) show "(\"only\",42)"
    check "C/tup" (show (right (tupC one))) "(\"only\",42)"
    check "CM/tup" (show (right (tupCM one))) "(\"only\",42)"
    checkIO "IC/tup" (tupIC one) (show . right) "(\"only\",42)"
    checkIO "ICM/tup" (tupICM one) (show . right) "(\"only\",42)"
    checkIO "T/tup" (tupT one) (show . right) "(\"only\",42)"
    checkIO "TM/tup" (tupTM one) (show . right) "(\"only\",42)"
    check "P/tupNum" (show (tupNumP "AB" 1 2)) "(65.0,66.0)"
    check "PM/tupNum" (show (tupNumPM "AB" 1 2)) "(65.0,66.0)"
    checkIO "I/tupNum" (tupNumI "AB" 1 2) show "(65.0,66.0)"
    checkIO "IM/tupNum" (tupNumIM "AB" 1 2) show "(65.0,66.0)"
    check "C/tupNum" (show (right (tupNumC "AB" 1 2))) "(65.0,66.0)"
    check "CM/tupNum" (show (right (tupNumCM "AB" 1 2))) "(65.0,66.0)"
    checkIO "IC/tupNum" (tupNumIC "AB" 1 2) (show . right) "(65.0,66.0)"
    checkIO "ICM/tupNum" (tupNumICM "AB" 1 2) (show . right) "(65.0,66.0)"
    checkIO "T/tupNum" (tupNumT "AB" 1 2) (show . right) "(65.0,66.0)"
    checkIO "TM/tupNum" (tupNumTM "AB" 1 2) (show . right) "(65.0,66.0)"
    check "P/tupNil" (show (tupNilP one "only")) "(Nothing,Nothing)"
    check "PM/tupNil" (show (tupNilPM one "only")) "(Nothing,Nothing)"
    checkIO "I/tupNil" (tupNilI one "only") show "(Nothing,Nothing)"
    checkIO "IM/tupNil" (tupNilIM one "only") show "(Nothing,Nothing)"
    check "C/tupNil" (show (right (tupNilC one "only"))) "(Nothing,Nothing)"
    check "CM/tupNil" (show (right (tupNilCM one "only"))) "(Nothing,Nothing)"
    checkIO "IC/tupNil" (tupNilIC one "only") (show . right) "(Nothing,Nothing)"
    checkIO "ICM/tupNil" (tupNilICM one "only") (show . right) "(Nothing,Nothing)"
    checkIO "T/tupNil" (tupNilT one "only") leftMsg "unknown error"
    checkIO "TM/tupNil" (tupNilTM one "only") leftMsg "unknown error"
    check "P/tup3" (third (tup3P obj)) "0"
    check "PM/tup3" (third (tup3PM obj)) "0"
    checkIO "I/tup3" (tup3I obj) third "0"
    checkIO "IM/tup3" (tup3IM obj) third "0"
    check "C/tup3" (third (right (tup3C obj))) "0"
    check "CM/tup3" (third (right (tup3CM obj))) "0"
    checkIO "IC/tup3" (tup3IC obj) (third . right) "0"
    checkIO "ICM/tup3" (tup3ICM obj) (third . right) "0"
    checkIO "T/tup3" (tup3T obj) (third . right) "0"
    checkIO "TM/tup3" (tup3TM obj) (third . right) "0"
    check "P/nest" (show (nestP obj "prs")) "[(1,\"a\"),(2,\"b\")]"
    check "PM/nest" (show (nestPM obj "prs")) "[(1,\"a\"),(2,\"b\")]"
    checkIO "I/nest" (nestI obj "prs") show "[(1,\"a\"),(2,\"b\")]"
    checkIO "IM/nest" (nestIM obj "prs") show "[(1,\"a\"),(2,\"b\")]"
    check "C/nest" (show (right (nestC obj "prs"))) "[(1,\"a\"),(2,\"b\")]"
    check "CM/nest" (show (right (nestCM obj "prs"))) "[(1,\"a\"),(2,\"b\")]"
    checkIO "IC/nest" (nestIC obj "prs") (show . right) "[(1,\"a\"),(2,\"b\")]"
    checkIO "ICM/nest" (nestICM obj "prs") (show . right) "[(1,\"a\"),(2,\"b\")]"
    checkIO "T/nest" (nestT obj "prs") (show . right) "[(1,\"a\"),(2,\"b\")]"
    checkIO "TM/nest" (nestTM obj "prs") (show . right) "[(1,\"a\"),(2,\"b\")]"
    check "P/mbTup" (show (mbTupP obj "pr")) "Just (1,\"a\")"
    check "PM/mbTup" (show (mbTupPM obj "pr")) "Just (1,\"a\")"
    checkIO "I/mbTup" (mbTupI obj "pr") show "Just (1,\"a\")"
    checkIO "IM/mbTup" (mbTupIM obj "pr") show "Just (1,\"a\")"
    check "C/mbTup" (show (right (mbTupC obj "pr"))) "Just (1,\"a\")"
    check "CM/mbTup" (show (right (mbTupCM obj "pr"))) "Just (1,\"a\")"
    checkIO "IC/mbTup" (mbTupIC obj "pr") (show . right) "Just (1,\"a\")"
    checkIO "ICM/mbTup" (mbTupICM obj "pr") (show . right) "Just (1,\"a\")"
    checkIO "T/mbTup" (mbTupT obj "pr") (show . right) "Just (1,\"a\")"
    checkIO "TM/mbTup" (mbTupTM obj "pr") (show . right) "Just (1,\"a\")"
    check "P/rec" (show (recP obj "rc")) "View {vName = \"ca\", vPort = 443.0, vTags = [\"x\",\"y\"], vAlias = Nothing}"
    check "PM/rec" (show (recPM obj "rc")) "View {vName = \"ca\", vPort = 443.0, vTags = [\"x\",\"y\"], vAlias = Nothing}"
    checkIO "I/rec" (recI obj "rc") show "View {vName = \"ca\", vPort = 443.0, vTags = [\"x\",\"y\"], vAlias = Nothing}"
    checkIO "IM/rec" (recIM obj "rc") show "View {vName = \"ca\", vPort = 443.0, vTags = [\"x\",\"y\"], vAlias = Nothing}"
    check "C/rec" (show (right (recC obj "rc"))) "View {vName = \"ca\", vPort = 443.0, vTags = [\"x\",\"y\"], vAlias = Nothing}"
    check "CM/rec" (show (right (recCM obj "rc"))) "View {vName = \"ca\", vPort = 443.0, vTags = [\"x\",\"y\"], vAlias = Nothing}"
    checkIO "IC/rec" (recIC obj "rc") (show . right) "View {vName = \"ca\", vPort = 443.0, vTags = [\"x\",\"y\"], vAlias = Nothing}"
    checkIO "ICM/rec" (recICM obj "rc") (show . right) "View {vName = \"ca\", vPort = 443.0, vTags = [\"x\",\"y\"], vAlias = Nothing}"
    checkIO "T/rec" (recT obj "rc") (show . right) "View {vName = \"ca\", vPort = 443.0, vTags = [\"x\",\"y\"], vAlias = Nothing}"
    checkIO "TM/rec" (recTM obj "rc") (show . right) "View {vName = \"ca\", vPort = 443.0, vTags = [\"x\",\"y\"], vAlias = Nothing}"
    check "P/hm" (hmShow (hmP obj "hm")) "(2,Just 2)"
    check "PM/hm" (hmShow (hmPM obj "hm")) "(2,Just 2)"
    checkIO "I/hm" (hmI obj "hm") hmShow "(2,Just 2)"
    checkIO "IM/hm" (hmIM obj "hm") hmShow "(2,Just 2)"
    check "C/hm" (hmShow (right (hmC obj "hm"))) "(2,Just 2)"
    check "CM/hm" (hmShow (right (hmCM obj "hm"))) "(2,Just 2)"
    checkIO "IC/hm" (hmIC obj "hm") (hmShow . right) "(2,Just 2)"
    checkIO "ICM/hm" (hmICM obj "hm") (hmShow . right) "(2,Just 2)"
    checkIO "T/hm" (hmT obj "hm") (hmShow . right) "(2,Just 2)"
    checkIO "TM/hm" (hmTM obj "hm") (hmShow . right) "(2,Just 2)"
    check "P/unit" (show (unitP obj "absent")) "()"
    check "PM/unit" (show (unitPM obj "absent")) "()"
    checkIO "I/unit" (unitI obj "absent") show "()"
    checkIO "IM/unit" (unitIM obj "absent") show "()"
    check "C/unit" (show (right (unitC obj "absent"))) "()"
    check "CM/unit" (show (right (unitCM obj "absent"))) "()"
    checkIO "IC/unit" (unitIC obj "absent") (show . right) "()"
    checkIO "ICM/unit" (unitICM obj "absent") (show . right) "()"
    checkIO "T/unit" (unitT obj "absent") leftMsg "unknown error"
    checkIO "TM/unit" (unitTM obj "absent") leftMsg "unknown error"
    check "K/int" (show intK) "3"
    check "K/str" (show strK) "\"ca\""
    check "K/num" (numShow numK) "(3.0,1.3835058055282164e19)"
    check "K/mbJ" (show mbJK) "Just 3"
    check "K/mbN" (show mbNK) "Nothing"
    check "K/list" (show listK) "[1,2,3]"
    check "K/tup" (show tupK) "(1,\"a\")"
    check "K/nest" (show nestK) "[(1,\"a\"),(2,\"b\")]"
    check "K/mbTup" (show mbTupK) "Just (1,\"a\")"
    check "K/rec" (show recK) "View {vName = \"ca\", vPort = 443.0, vTags = [\"x\",\"y\"], vAlias = Nothing}"
    check "K/hm" (hmShow hmK) "(2,Just 2)"
    check "K/unit" (show unitK) "()"
    check "IT/str" (show (strIT "ab cd" "%a+")) "[\"ab\",\"cd\"]"
    check "ITM/str" (show (strITM "ab cd" "%a+")) "[\"ab\",\"cd\"]"
    check "IT/int" (show (intIT "a1b22" "()%d")) "[2,4,5]"
    check "ITM/int" (show (intITM "a1b22" "()%d")) "[2,4,5]"
    check "IT/num" (numsShow (numIT "a1b22" "()%d")) "([2.0,4.0,5.0],9.223372036854776e18)"
    check "ITM/num" (numsShow (numITM "a1b22" "()%d")) "([2.0,4.0,5.0],9.223372036854776e18)"
    check "IT/tup" (show (tupIT "a1b22" "(%a)()")) "[(\"a\",2),(\"b\",4)]"
    check "ITM/tup" (show (tupITM "a1b22" "(%a)()")) "[(\"a\",2),(\"b\",4)]"
    check "IT/tupNum" (show (tupNumIT "a1b22" "()%d+()")) "[(2.0,3.0),(4.0,6.0)]"
    check "ITM/tupNum" (show (tupNumITM "a1b22" "()%d+()")) "[(2.0,3.0),(4.0,6.0)]"
    check "IT/first" (show (firstIT "a1b22" "(%a)(%d+)")) "[\"a\",\"b\"]"
    check "ITM/first" (show (firstITM "a1b22" "(%a)(%d+)")) "[\"a\",\"b\"]"
    let pending = tupI one
    again <- mapM id [pending, pending]
    check "I/tup as a value, run twice" (show again) "[(\"only\",42),(\"only\",42)]"
    let pendingM = mbJIM obj "i"
    againM <- mapM id [pendingM, pendingM]
    check "IM/mbJ as a value, run twice" (show againM) "[Just 3,Just 3]"
    check "C/raise" (leftMsg (raiseC "boom" 0)) "boom"
    checkIO "IC/raise" (raiseIC "bang" 0) leftMsg "bang"
    check "CM/raise" (mentions "bad argument" (formatCM "%d" "x")) "bad argument"
    check "CM/no raise" (show (formatCM "%s!" "x")) "Right (Just \"x!\")"
    checkIO "ICM/raise" (formatICM "%d" "x") (mentions "bad argument") "bad argument"
    checkIO "T/failure message" (selectT 1 Nothing "no such thing") leftMsg "no such thing"
    checkIO "T/success" (selectT 1 (Just 5) "five") (show . right) "(5,\"five\")"
    checkIO "TM/failure" (rawgetTM obj "absent") leftMsg "unknown error"
    nilP <- try (putStrLn (numShow (numP obj "absent")))
    check "P/num nil raises" (mentions "arithmetic" nilP) "arithmetic"
    nilPM <- try (putStrLn (numShow (numPM obj "absent")))
    check "PM/num nil raises" (mentions "arithmetic" nilPM) "arithmetic"
    nilI <- try (numI obj "absent")
    check "I/num nil raises" (mentions "arithmetic" nilI) "arithmetic"
    nilIM <- try (numIM obj "absent")
    check "IM/num nil raises" (mentions "arithmetic" nilIM) "arithmetic"
    nilC <- try (putStrLn (numShow (right (numC obj "absent"))))
    check "C/num nil raises" (mentions "declared Number but the host returned nil" nilC) "declared Number but the host returned nil"
    nilCM <- try (putStrLn (numShow (right (numCM obj "absent"))))
    check "CM/num nil raises" (mentions "declared Number but the host returned nil" nilCM) "declared Number but the host returned nil"
    nilIC <- try (numIC obj "absent")
    check "IC/num nil raises" (mentions "declared Number but the host returned nil" nilIC) "declared Number but the host returned nil"
    nilICM <- try (numICM obj "absent")
    check "ICM/num nil raises" (mentions "declared Number but the host returned nil" nilICM) "declared Number but the host returned nil"
    checkIO "T/num nil is the failure" (numT obj "absent") leftMsg "unknown error"
    checkIO "TM/num nil is the failure" (numTM obj "absent") leftMsg "unknown error"
    short <- try (putStrLn (show (shortP one "only")))
    check "P/tup missing value raises" (mentions "element 1 of the tuple declared (String, Int)" short) "element 1 of the tuple declared (String, Int)"
    shortM <- try (putStrLn (show (right (shortCM one "only"))))
    check "CM/tup missing value raises" (mentions "element 1 of the tuple declared (String, Int)" shortM) "element 1 of the tuple declared (String, Int)"
