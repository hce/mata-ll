-- GHC cgrun058: Church-style integers, mutual recursion

data MInt = Zero | Succ MInt | Pred MInt
    deriving (Show)

tn :: Int -> MInt
tn x = if x < 0 then Pred (tn (x + 1)) else if x == 0 then Zero else Succ (tn (x - 1))

ti :: MInt -> Int
ti Zero = 0
ti (Succ x) = 1 + ti x
ti (Pred x) = ti x - 1

myMul :: MInt -> MInt -> MInt
myMul x y = tn (ti x * ti y)

testi :: Int -> Int -> Bool
testi x y = ti (myMul (tn x) (tn y)) /= x * y

test :: [(Int, Int, Int, Int)]
test = [(x, y, ti (myMul (tn x) (tn y)), x * y) | x <- [-100, -99, -98, -97, -2, -1, 0, 1, 2, 97, 98, 99, 100], y <- [-100, -99, -98, -97, -2, -1, 0, 1, 2, 97, 98, 99, 100], testi x y]

main :: IO ()
main = putStrLn (show test)
