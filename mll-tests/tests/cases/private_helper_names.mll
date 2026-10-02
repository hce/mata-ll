import PrivateHelperLib

helper :: Int -> Int
helper x = x + 100

step :: String
step = "local"

main :: IO ()
main = do
    print (scaled 1)
    print (shifted 1)
    print (helper 1)
    putStrLn step
