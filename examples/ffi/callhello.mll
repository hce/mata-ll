data HandleMeOperation = Addition       as "+"
        | Subtraction    as "-"
        | Multiplication as "*"
        | Division       as "/"
        | Power          as "^"
    deriving (Show, LuaDict)

data HandleMeParams = HandleMeParams
        { hmpOperation   as "operation" :: HandleMeOperation
        , hmpOperands    as "operands"  :: [Integer] }
    deriving (Show, LuaDict)

data HandleMeRet = HandleMeRet
        { hmrSuccess     as "success"   :: Bool
        , hmrValue       as "value"     :: Integer }
    deriving (Show, LuaDict)

handleMe :: HandleMeParams -> LuaPure "handle_me" HandleMeRet

export doit :: IO ()
doit = do
    print $ handleMe $ HandleMeParams Division [42, 3]
    print $ handleMe $ HandleMeParams Power [2, 10]
