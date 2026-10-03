import EntListA (T(..), U, mkU, unU, V(..), mkV2, showV, C(..), R, rx, mkR, D, useD)
import EntListB hiding (Color(..))
import qualified EntListB as LB

data Color = Red | Blue
    deriving Show

data Mine = Mine Int

instance C Mine where
    cm (Mine n) = "mine " <> show n

describeT :: T -> String
describeT (T1 n) = "T1 " <> show n
describeT T2 = "T2"

main :: IO ()
main = do
    putStrLn (describeT (T1 3) <> " " <> describeT T2)
    print (unU (mkU 4))
    putStrLn (showV (V1 1) <> " " <> showV (mkV2 2))
    putStrLn (cm (5 :: Int) <> " " <> cm (Mine 6))
    print (rx (mkR 7 8))
    print (useD 9)
    print (Red, Blue, Dark, Light)
    putStrLn (paint LB.Red)
