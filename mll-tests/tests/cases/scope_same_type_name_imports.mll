import ScopeCfgA
import ScopeCfgB

main :: IO ()
main = do
    assert (showA defaultA == "A1") "ScopeCfgA's Config"
    assert (showB defaultB == "Bx") "ScopeCfgB's Config"
    putStrLn (showA defaultA)
    putStrLn (showB defaultB)
