import Data.IORef

raiseHost :: String -> LuaIO "error" ()
guardedHost :: String -> LuaIOCatch "error" (Either String ())
seedHost :: Int -> LuaIO "math.randomseed" ()

isLeft :: Either String a -> Bool
isLeft (Left _) = True
isLeft (Right _) = False

capturedInside :: Either String (Either String ()) -> Bool
capturedInside (Right (Left _)) = True
capturedInside _ = False

main :: IO ()
main = do
    a <- try $ raiseHost "prefix dollar"
    assert (isLeft a) "try $ over a raising host action is Left"
    rs <- mapM try [raiseHost "first", seedHost 1, raiseHost "third"]
    assert (map isLeft rs == [True, False, True]) "mapM try over host actions"
    hits <- newIORef (0 :: Int)
    raiseHost "backtick" `catch` (\_ -> modifyIORef hits (+ 1))
    seedHost 2 `catch` (\_ -> modifyIORef hits (+ 10))
    n <- readIORef hits
    assert (n == 1) "backtick catch runs the handler only on a raise"
    let pending = guardedHost "captured"
    g1 <- try pending
    g2 <- try pending
    assert (capturedInside g1 && capturedInside g2) "LuaIOCatch value run twice under try captures each time"
    let handle = (`catch` (\_ -> modifyIORef hits (+ 100)))
    handle (raiseHost "section")
    handle (raiseHost "section again")
    m <- readIORef hits
    assert (m == 201) "a catch section is an ordinary function"
