data HandleMeOperation = Addition       as "+"
        | Subtraction    as "-"
        | Multiplication as "*"
        | Division       as "/"
        | Power          as "^"
    deriving (Show, LuaDict)

data HandleMeParams = HandleMeParams
        { hmpMessage     as "message"   :: String
        , hmpOperation   as "operation" :: HandleMeOperation
        , hmpOperands    as "operands"  :: [Integer] }
    deriving (Show, LuaDict)

data HandleMeRet = HandleMeRet
        { hmrSuccess     as "success"   :: Bool
        , hmrValue       as "value"     :: Integer }
    deriving (Show, LuaDict)

handleMe :: HandleMeParams -> LuaPure "handle_me" HandleMeRet

export doit :: IO ()
doit = do
    mapM_ (\(a, b, c) -> print $ handleMe $ HandleMeParams a b c) [("Hi", Division, [42, 3]), ("Ho", Power, [2, 10])]
