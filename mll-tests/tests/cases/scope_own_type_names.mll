import Prelude hiding (Ordering)
import ScopeCfgA (showA, defaultA)
import ScopeCfgB (showB, defaultB)

data Config = Mine Bool

data Ordering = Small | Big
    deriving Show

describe :: Config -> String
describe (Mine b) = show b

rank :: Int -> Ordering
rank n = if n < 3 then Small else Big

main :: IO ()
main = do
    assert (showA defaultA == "A1") "an imported module's Config"
    assert (showB defaultB == "Bx") "another imported module's Config"
    assert (describe (Mine True) == "True") "the root's Config"
    assert (show (rank 1) == "Small") "the root's Ordering"
    assert (show (compare 1 2) == "LT") "the Prelude's Ordering is untouched"
    putStrLn (describe (Mine True))
    print (rank 5)
    print (compare 1 2)
