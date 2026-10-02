import Prelude hiding (lookup)
import qualified Prelude as P

capped :: Int -> Int
capped error = if error > 100 then P.error "too large" else error + 1

spread :: Int -> [Int]
spread concatMap = P.concatMap (\x -> [x, concatMap]) [x | x <- [1, 2]]

upTo :: Int -> [Int]
upTo enumFromTo = P.enumFromTo 1 enumFromTo ++ [enumFromTo .. enumFromTo + 1]

main :: IO ()
main = do
    assert (capped 1 == 2) "the Prelude's error through its alias, under a parameter named error"
    assert (spread 9 == [1, 9, 2, 9]) "the Prelude's concatMap through its alias, under a parameter named concatMap"
    assert (upTo 3 == [1, 2, 3, 3, 4]) "the Prelude's enumFromTo through its alias, under a parameter named enumFromTo"
    print (capped 1, spread 9, upTo 3)
